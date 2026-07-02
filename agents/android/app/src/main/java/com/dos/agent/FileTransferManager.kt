package com.dos.agent

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Environment
import android.os.PowerManager
import android.provider.OpenableColumns
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.localbroadcastmanager.content.LocalBroadcastManager
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import java.io.*
import java.text.SimpleDateFormat
import java.util.*
import java.util.concurrent.ConcurrentHashMap

enum class TransferDirection { SEND, RECEIVE }

enum class TransferStatus {
    QUEUED, CONNECTING, TRANSFERRING, COMPLETED, FAILED, CANCELLED, PAUSED
}

data class TransferRecord(
    val id: String,
    val filename: String,
    val totalBytes: Long,
    val direction: TransferDirection,
    val peerIp: String,
    val timestamp: Long,
    val status: TransferStatus,
    val averageSpeed: Double,
    val durationMs: Long
)

class TransferState(
    val id: String = UUID.randomUUID().toString(),
    val filename: String,
    val totalBytes: Long,
    val direction: TransferDirection,
    val peerIp: String = "",
    var downloadUrl: String = "",
    var fileUri: Uri? = null,
    var transferredBytes: Long = 0L,
    val startTime: Long = System.currentTimeMillis(),
    var status: TransferStatus = TransferStatus.QUEUED,
    var errorMessage: String = "",
    var speedSamples: MutableList<Pair<Long, Long>> = mutableListOf(),
    var job: Job? = null,
    var cancelFlag: Boolean = false,
    var pairingCode: String = "",
    var isTextTransfer: Boolean = false,
    var textContent: String = "",
    var existingBytes: Long = 0L,
    var retryCount: Int = 0
) {
    val elapsedMs: Long
        get() = if (status == TransferStatus.TRANSFERRING || status == TransferStatus.COMPLETED)
            System.currentTimeMillis() - startTime else 0L

    val instantSpeedBps: Double
        get() {
            val recent = speedSamples.takeLast(4)
            if (recent.size < 2) return 0.0
            val duration = recent.last().first - recent.first().first
            val bytes = recent.last().second - recent.first().second
            return if (duration > 0) bytes.toDouble() / duration * 1000.0 else 0.0
        }

    val averageSpeedBps: Double
        get() {
            val e = elapsedMs
            return if (e > 0 && transferredBytes > 0) transferredBytes.toDouble() / e * 1000.0 else 0.0
        }

    val progressPercent: Float
        get() = if (totalBytes > 0) (transferredBytes.toFloat() / totalBytes * 100).coerceIn(0f, 100f) else 0f

    val etaMs: Long
        get() {
            val avg = averageSpeedBps
            return if (avg > 0) ((totalBytes - transferredBytes) / avg * 1000).toLong() else -1L
        }

    fun recordProgress(bytesTransferred: Long) {
        speedSamples.add(Pair(System.currentTimeMillis(), bytesTransferred))
        if (speedSamples.size > 20) speedSamples.removeAt(0)
    }

    fun toRecord(): TransferRecord = TransferRecord(
        id = id,
        filename = filename,
        totalBytes = totalBytes,
        direction = direction,
        peerIp = peerIp,
        timestamp = startTime,
        status = status,
        averageSpeed = averageSpeedBps,
        durationMs = if (status == TransferStatus.COMPLETED) System.currentTimeMillis() - startTime else 0L
    )
}

object FileTransferManager {
    private const val TAG = "FileTransferManager"
    private const val HISTORY_FILE = "transfer_history.json"
    private const val PROGRESS_INTERVAL_MS = 500L
    private const val SPEED_SAMPLE_INTERVAL_MS = 500L
    private const val MAX_RETRIES = 3
    const val ACTION_PROGRESS = "PDOS_TRANSFER_PROGRESS"
    private const val ACTION_HISTORY_UPDATE = "PDOS_TRANSFER_HISTORY"

    private val activeTransfers = ConcurrentHashMap<String, TransferState>()
    private val history = java.util.Collections.synchronizedList(mutableListOf<TransferRecord>())
    private var historyLoaded = false
    private var contextRef: Context? = null
    private var scope: CoroutineScope? = null

    private var fileServer: java.nio.channels.ServerSocketChannel? = null
    private var wifiLock: WifiManager.WifiLock? = null
    private var wakeLock: PowerManager.WakeLock? = null

    private fun acquireLocks(context: Context) {
        try {
            if (wifiLock == null) {
                val wifi = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
                wifiLock = wifi.createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "PDOS_Transfer")
            }
            wifiLock?.acquire()
        } catch (e: Exception) {
            Log.w(TAG, "Failed to acquire WifiLock: ${e.message}")
        }
        try {
            if (wakeLock == null) {
                val power = context.applicationContext.getSystemService(Context.POWER_SERVICE) as PowerManager
                wakeLock = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "PDOS_Transfer")
            }
            wakeLock?.acquire(300_000) // 5 min max
        } catch (e: Exception) {
            Log.w(TAG, "Failed to acquire WakeLock: ${e.message}")
        }
    }

    private fun releaseLocks() {
        try {
            wifiLock?.let {
                if (it.isHeld) it.release()
            }
        } catch (_: Exception) {}
        try {
            wakeLock?.let {
                if (it.isHeld) it.release()
            }
        } catch (_: Exception) {}
    }

    fun init(context: Context) {
        contextRef = context.applicationContext
        scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
        loadHistory()
        startFileServer()
        // Clean up stale partial files from previous runs
        sweepPartialFiles()
    }

    private fun sweepPartialFiles() {
        try {
            val ctx = contextRef ?: return
            // Cache dir temp files
            ctx.cacheDir.listFiles()?.forEach { f ->
                if (f.name.startsWith("upload_") || f.name.startsWith("partial_")) f.delete()
            }
            // Download dir partial markers
            val downloadDir = SettingsManager.getDownloadDir()
            downloadDir.listFiles()?.forEach { f ->
                if (f.name.startsWith(".partial_")) f.delete()
            }
        } catch (_: Exception) {}
    }

    fun startFileServer() {
        if (fileServer != null && fileServer!!.socket()?.isBound == true) return
        try {
            val downloadsDir = SettingsManager.getDownloadDir()
            NativeTransferEngine.startServer(7894, downloadsDir.absolutePath)
        } catch (e: java.net.BindException) {
            android.util.Log.w("FileTransferManager", "File server already bound, skipping")
        }
    }

    fun cleanup() {
        scope?.cancel()
        fileServer?.close()
        releaseLocks()
    }

    fun getActiveTransfers(): List<TransferState> = activeTransfers.values.toList()

    fun getHistory(): List<TransferRecord> = history.toList().sortedByDescending { it.timestamp }

    fun clearHistory() {
        history.clear()
        saveHistory()
        broadcastHistoryUpdate()
    }

    fun getTransfer(id: String): TransferState? = activeTransfers[id]

    fun startDownload(url: String, filename: String, totalBytes: Long, peerIp: String = "", useEncryption: Boolean = false): String {
        val state = TransferState(
            filename = filename,
            totalBytes = totalBytes,
            direction = TransferDirection.RECEIVE,
            peerIp = peerIp,
            downloadUrl = url,
            status = TransferStatus.QUEUED
        )
        activeTransfers[state.id] = state
        broadcastProgress(state)
        state.status = TransferStatus.CONNECTING
        state.job = scope?.launch {
            try {
                if (useEncryption && url.startsWith("http:")) {
                    state.downloadUrl = url.replace("http:", "https:")
                }
                downloadFile(state)
            } catch (e: Exception) {
                Log.e(TAG, "Download failed: ${e.message}")
                state.status = TransferStatus.FAILED
                state.errorMessage = e.message ?: "Unknown error"
                broadcastProgress(state)
                addToHistory(state)
            }
        }
        return state.id
    }

    fun startUpload(fileUri: Uri, filename: String, totalBytes: Long, macIp: String, useEncryption: Boolean = false): String {
        val state = TransferState(
            filename = filename,
            totalBytes = totalBytes,
            direction = TransferDirection.SEND,
            peerIp = macIp,
            fileUri = fileUri,
            status = TransferStatus.QUEUED
        )
        activeTransfers[state.id] = state
        broadcastProgress(state)
        state.status = TransferStatus.CONNECTING
        state.job = scope?.launch {
            try {
                uploadFile(state, useEncryption)
            } catch (e: Exception) {
                Log.e(TAG, "Upload failed: ${e.message}")
                state.status = TransferStatus.FAILED
                state.errorMessage = e.message ?: "Unknown error"
                broadcastProgress(state)
                addToHistory(state)
            }
        }
        return state.id
    }

    fun resendTransfer(record: TransferRecord): String {
        val ctx = contextRef ?: return ""
        if (record.direction != TransferDirection.SEND) return ""
        val state = TransferState(
            filename = record.filename,
            totalBytes = record.totalBytes,
            direction = TransferDirection.SEND,
            peerIp = record.peerIp,
            status = TransferStatus.QUEUED
        )
        activeTransfers[state.id] = state
        broadcastProgress(state)
        scope?.launch {
            try {
                uploadFile(state, SettingsManager.encryptionEnabled)
            } catch (e: Exception) {
                state.status = TransferStatus.FAILED
                state.errorMessage = e.message ?: "Resend failed"
                broadcastProgress(state)
                addToHistory(state)
            }
        }
        return state.id
    }

    fun startTextTransfer(text: String, macIp: String, useEncryption: Boolean = false): String {
        val ctx = contextRef ?: return ""
        val filename = "clipboard_${System.currentTimeMillis()}.txt"
        val tempFile = File(ctx.cacheDir, filename)
        tempFile.writeText(text)
        val fileUri = Uri.fromFile(tempFile)
        val state = TransferState(
            filename = filename,
            totalBytes = text.length.toLong(),
            direction = TransferDirection.SEND,
            peerIp = macIp,
            fileUri = fileUri,
            status = TransferStatus.QUEUED,
            isTextTransfer = true,
            textContent = text
        )
        activeTransfers[state.id] = state
        broadcastProgress(state)
        state.status = TransferStatus.CONNECTING
        state.job = scope?.launch {
            try {
                uploadFile(state, useEncryption)
            } catch (e: Exception) {
                Log.e(TAG, "Text send failed: ${e.message}")
                state.status = TransferStatus.FAILED
                state.errorMessage = e.message ?: "Unknown error"
                broadcastProgress(state)
                addToHistory(state)
            }
        }
        return state.id
    }

    fun generatePairingCode(): String {
        return "%04d".format((1000..9999).random())
    }

    fun startMultipleUpload(fileUris: List<Uri>, filenames: List<String>, totalBytes: Long, macIp: String, useEncryption: Boolean = false): List<String> {
        val ids = mutableListOf<String>()
        for (i in fileUris.indices) {
            val id = startUpload(fileUris[i], filenames.getOrElse(i) { "file_$i" }, 0L, macIp, useEncryption)
            ids.add(id)
        }
        return ids
    }

    fun cancelTransfer(id: String) {
        val state = activeTransfers[id] ?: return
        state.cancelFlag = true
        state.status = TransferStatus.CANCELLED
        state.job?.cancel()
        // Delete partial files
        val ctx = contextRef
        if (ctx != null) {
            val partialFile = File(ctx.cacheDir, "partial_${state.id}_${state.filename}")
            if (partialFile.exists()) partialFile.delete()
            val tempFile = File(ctx.cacheDir, "upload_${state.id}_${state.filename}")
            if (tempFile.exists()) tempFile.delete()
            val downloadDir = SettingsManager.getDownloadDir()
            val outFile = File(downloadDir, ".partial_${state.filename}")
            if (outFile.exists()) outFile.delete()
        }
        broadcastProgress(state)
        addToHistory(state)
        activeTransfers.remove(id)
    }

    fun formatSpeed(bps: Double): String {
        return when {
            bps >= 1_000_000 -> "%.1f MB/s".format(bps / 1_000_000)
            bps >= 1_000 -> "%.0f KB/s".format(bps / 1_000)
            else -> "%.0f B/s".format(bps)
        }
    }

    fun formatSize(bytes: Long): String {
        return when {
            bytes >= 1_000_000_000 -> "%.2f GB".format(bytes / 1_000_000_000.0)
            bytes >= 1_000_000 -> "%.1f MB".format(bytes / 1_000_000.0)
            bytes >= 1_000 -> "%.0f KB".format(bytes / 1_000.0)
            else -> "$bytes B"
        }
    }

    fun formatDuration(ms: Long): String {
        if (ms <= 0) return "--"
        val totalSec = ms / 1000
        val hours = totalSec / 3600
        val mins = (totalSec % 3600) / 60
        val secs = totalSec % 60
        return when {
            hours > 0 -> "%d:%02d:%02d".format(hours, mins, secs)
            mins > 0 -> "%d:%02d".format(mins, secs)
            else -> "%ds".format(secs)
        }
    }

    fun formatEta(ms: Long): String {
        if (ms <= 0) return "--"
        return formatDuration(ms)
    }

    private fun downloadFile(state: TransferState) {
        val ctx = contextRef ?: return
        val url = state.downloadUrl
        val filename = state.filename
        val speedLimit = SettingsManager.speedLimitBps
        val shouldZip = SettingsManager.zipOnReceive

        val downloadsDir = SettingsManager.getDownloadDir()
        val outFile = if (shouldZip && state.direction == TransferDirection.RECEIVE) {
            File(downloadsDir, "$filename.zip")
        } else {
            File(downloadsDir, filename)
        }
        outFile.parentFile?.mkdirs()

        // Check for partial file for resume
        val partialFile = File(downloadsDir, ".partial_$filename")
        val existingBytes = if (partialFile.exists()) partialFile.length() else 0L

        // Check available storage
        val freeBytes = NetworkUtil.getStorageFreeBytes(ctx, downloadsDir)
        if (freeBytes < state.totalBytes) {
            releaseLocks()
            state.status = TransferStatus.FAILED
            state.errorMessage = "Not enough storage (~${formatSize(freeBytes)} free, need ${formatSize(state.totalBytes)})"
            broadcastProgress(state)
            addToHistory(state)
            return
        }

        state.status = TransferStatus.TRANSFERRING
        broadcastProgress(state)
        acquireLocks(ctx)

        // Capability handshake before download
        val peerIp = state.peerIp
        if (peerIp.isNotEmpty()) {
            val peerCaps = NioTransfer.performHandshake(host = peerIp, port = if (SettingsManager.encryptionEnabled) 8443 else 8080)
            if (peerCaps != null) {
                Log.i(TAG, "Handshake with ${peerCaps.node_id}: storage=${peerCaps.hardware.storage_type}, link=${peerCaps.network.interface_type} ${peerCaps.network.link_speed_mbps}Mbps, resume=${peerCaps.features.resume}, stream-dir=${peerCaps.features.streaming_directory}")
            } else {
                Log.w(TAG, "Handshake failed, proceeding with default")
            }
        }

        var result: TransferResult
        var attempt = 0
        while (true) {
            result = NioTransfer.httpDownloadWithResume(
                url = url,
                outputFile = outFile,
                existingBytes = existingBytes,
                maxBytesPerSecond = speedLimit,
                progressCallback = { total ->
                    if (state.cancelFlag) throw java.io.IOException("Cancelled")
                    state.transferredBytes = total
                    state.recordProgress(total)
                    if (total > 0 && total < state.totalBytes) {
                        try {
                            partialFile.parentFile?.mkdirs()
                            partialFile.writeText(total.toString())
                        } catch (_: Exception) {}
                    }
                    broadcastProgress(state)
                    updateTransferNotification(ctx, state)
                }
            )
            if (result.success) break
            attempt++
            if (attempt >= MAX_RETRIES || state.cancelFlag) {
                state.status = TransferStatus.FAILED
                state.errorMessage = result.errorMessage
                state.retryCount = attempt
                broadcastProgress(state)
                addToHistory(state)
                return
            }
            state.retryCount = attempt
            Log.i(TAG, "Retry $attempt/$MAX_RETRIES for $filename: ${result.errorMessage}")
            broadcastProgress(state)
            Thread.sleep(1000L * attempt)
        }

        // If was ZIP on receive, unzip and delete archive
        if (shouldZip && outFile.name.endsWith(".zip")) {
            NioTransfer.unzipToDir(outFile, downloadsDir)
            outFile.delete()
        }

        // Clean up partial marker
        partialFile.delete()

        releaseLocks()
        state.status = TransferStatus.COMPLETED
        state.transferredBytes = result.bytesTransferred
        broadcastProgress(state)
        addToHistory(state)

        ctx.sendBroadcast(Intent(Intent.ACTION_MEDIA_SCANNER_SCAN_FILE).apply {
            data = Uri.fromFile(outFile)
        })

        showCompletionNotification(ctx, filename, state.direction)
    }

    private fun uploadFile(state: TransferState, useEncryption: Boolean = false) {
        val ctx = contextRef ?: return
        val fileUri = state.fileUri ?: throw Exception("No file URI")
        val macIp = state.peerIp
        val filename = state.filename
        val speedLimit = SettingsManager.speedLimitBps
        val freeBytes = NetworkUtil.getStorageFreeBytes(ctx, ctx.cacheDir)
        if (freeBytes in 1..(1024 * 1024)) {
            releaseLocks()
            state.status = TransferStatus.FAILED
            state.errorMessage = "Low storage space (~${FileTransferManager.formatSize(freeBytes)} free)"
            broadcastProgress(state)
            addToHistory(state)
            return
        }

        val fileSize = try {
            ctx.contentResolver.openFileDescriptor(fileUri, "r")?.statSize ?: state.totalBytes
        } catch (e: SecurityException) {
            releaseLocks()
            state.status = TransferStatus.FAILED
            state.errorMessage = "Permission denied: ${e.message}"
            broadcastProgress(state)
            addToHistory(state)
            return
        }
        if (fileSize <= 0) {
            releaseLocks()
            state.status = TransferStatus.FAILED
            state.errorMessage = "Cannot determine file size"
            broadcastProgress(state)
            addToHistory(state)
            return
        }

        // Copy content URI to temp file for FileChannel access
        val tempFile = File(ctx.cacheDir, "upload_${state.id}_$filename")
        tempFile.parentFile?.mkdirs()
        try {
            ctx.contentResolver.openInputStream(fileUri)?.use { input ->
                tempFile.outputStream().use { output ->
                    input.copyTo(output)
                }
            } ?: throw Exception("Cannot open file")
        } catch (e: Exception) {
            if (tempFile.exists()) tempFile.delete()
            releaseLocks()
            state.status = TransferStatus.FAILED
            state.errorMessage = "Cannot access file: ${e.message}"
            broadcastProgress(state)
            addToHistory(state)
            return
        }

        state.status = TransferStatus.TRANSFERRING
        broadcastProgress(state)
        acquireLocks(ctx)

        // Capability handshake before upload
        val peerCaps = NioTransfer.performHandshake(host = macIp, port = if (useEncryption) 8443 else 8080)
        if (peerCaps != null) {
            Log.i(TAG, "Handshake with ${peerCaps.node_id}: storage=${peerCaps.hardware.storage_type}, link=${peerCaps.network.interface_type} ${peerCaps.network.link_speed_mbps}Mbps, resume=${peerCaps.features.resume}, stream-dir=${peerCaps.features.streaming_directory}")
        } else {
            Log.w(TAG, "Handshake failed, proceeding with default")
        }

        var result: TransferResult
        try {
            // Try HTTP upload first (works on WiFi, USB tether)
            result = NioTransfer.httpUpload(
                file = tempFile,
                filename = filename,
                host = macIp,
                port = if (useEncryption) 8443 else 8080,
                path = "/api/receive-file",
                useTls = useEncryption,
                maxBytesPerSecond = speedLimit,
                progressCallback = { total ->
                    if (state.cancelFlag) throw java.io.IOException("Cancelled")
                    state.transferredBytes = total
                    state.recordProgress(total)
                    broadcastProgress(state)
                    updateTransferNotification(ctx, state)
                }
            )
            
            // If HTTP fails, fall back to tunnel (works on hotspot where direct TCP is blocked)
            if (!result.success) {
                Log.w(TAG, "HTTP upload failed (${result.errorMessage}), trying tunnel on port 7896...")
                val tunnelResult = TunnelClient.sendFileToMac(
                    macIp = macIp,
                    file = tempFile,
                    filename = filename,
                    tunnelPort = 7896,
                    progressCallback = { total: Long ->
                        if (state.cancelFlag) throw java.io.IOException("Cancelled")
                        state.transferredBytes = total
                        state.recordProgress(total)
                        broadcastProgress(state)
                        updateTransferNotification(ctx, state)
                    }
                )
                if (tunnelResult) {
                    result = TransferResult(true, fileSize, "", "")
                }
            }
        } finally {
            if (tempFile.exists()) tempFile.delete()
        }

        if (!result.success) {
            releaseLocks()
            state.status = TransferStatus.FAILED
            state.errorMessage = result.errorMessage
            broadcastProgress(state)
            addToHistory(state)
            return
        }

        releaseLocks()
        state.status = TransferStatus.COMPLETED
        state.transferredBytes = result.bytesTransferred
        broadcastProgress(state)
        addToHistory(state)

        showCompletionNotification(ctx, filename, state.direction)
    }

    private fun updateTransferNotification(ctx: Context, state: TransferState) {
        val channelId = if (state.direction == TransferDirection.SEND) "pdos_upload_channel" else "pdos_download_channel"
        val speedStr = formatSpeed(state.instantSpeedBps)
        val progress = state.progressPercent
        val transferred = formatSize(state.transferredBytes)
        val total = formatSize(state.totalBytes)
        val etaStr = formatEta(state.etaMs)
        val groupKey = if (state.direction == TransferDirection.SEND) "pdos_uploads" else "pdos_downloads"

        val notif = NotificationCompat.Builder(ctx, channelId)
            .setContentTitle(state.filename)
            .setContentText("$transferred / $total  ·  $speedStr  ·  ETA $etaStr")
            .setSmallIcon(if (state.direction == TransferDirection.SEND) android.R.drawable.stat_sys_upload else android.R.drawable.stat_sys_download)
            .setProgress(100, progress.toInt(), false)
            .setOngoing(true)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setGroup(groupKey)
            .build()

        // Summary notification for group
        val summary = NotificationCompat.Builder(ctx, channelId)
            .setContentTitle("File Transfers")
            .setContentText("${getActiveTransfers().size} active")
            .setSmallIcon(R.drawable.ic_file_generic)
            .setGroup(groupKey)
            .setGroupSummary(true)
            .build()

        try {
            val mgr = NotificationManagerCompat.from(ctx)
            mgr.notify(state.id.hashCode(), notif)
            mgr.notify(groupKey.hashCode(), summary)
        } catch (_: Exception) {}
    }

    private fun showCompletionNotification(ctx: Context, filename: String, direction: TransferDirection) {
        val notif = NotificationCompat.Builder(ctx, "pdos_download_channel")
            .setContentTitle(if (direction == TransferDirection.SEND) "File Sent" else "Download Complete")
            .setContentText(filename)
            .setSmallIcon(if (direction == TransferDirection.SEND) android.R.drawable.stat_sys_upload_done else android.R.drawable.stat_sys_download_done)
            .setAutoCancel(true)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .build()
        try {
            NotificationManagerCompat.from(ctx).notify(System.currentTimeMillis().toInt(), notif)
        } catch (_: Exception) {}
    }

    private fun broadcastProgress(state: TransferState) {
        val ctx = contextRef ?: return
        val intent = Intent(ACTION_PROGRESS).apply {
            putExtra("transferId", state.id)
            putExtra("filename", state.filename)
            putExtra("totalBytes", state.totalBytes)
            putExtra("transferredBytes", state.transferredBytes)
            putExtra("progressPercent", state.progressPercent)
            putExtra("instantSpeedBps", state.instantSpeedBps)
            putExtra("averageSpeedBps", state.averageSpeedBps)
            putExtra("status", state.status.name)
            putExtra("direction", state.direction.name)
            putExtra("etaMs", state.etaMs)
            putExtra("peerIp", state.peerIp)
            putExtra("errorMessage", state.errorMessage)
        }
        try {
            LocalBroadcastManager.getInstance(ctx).sendBroadcast(intent)
        } catch (_: Exception) {}
    }

    private fun addToHistory(state: TransferState) {
        val record = state.toRecord()
        history.add(0, record)
        if (history.size > 200) history.removeAt(history.lastIndex)
        activeTransfers.remove(state.id)
        saveHistory()
        broadcastHistoryUpdate()
    }

    private fun broadcastHistoryUpdate() {
        val ctx = contextRef ?: return
        try {
            LocalBroadcastManager.getInstance(ctx).sendBroadcast(Intent(ACTION_HISTORY_UPDATE))
        } catch (_: Exception) {}
    }

    private fun saveHistory() {
        val ctx = contextRef ?: return
        try {
            val arr = JSONArray()
            history.forEach { record ->
                arr.put(JSONObject().apply {
                    put("id", record.id)
                    put("filename", record.filename)
                    put("totalBytes", record.totalBytes)
                    put("direction", record.direction.name)
                    put("peerIp", record.peerIp)
                    put("timestamp", record.timestamp)
                    put("status", record.status.name)
                    put("averageSpeed", record.averageSpeed)
                    put("durationMs", record.durationMs)
                })
            }
            ctx.openFileOutput(HISTORY_FILE, Context.MODE_PRIVATE).use {
                it.write(arr.toString(2).toByteArray())
            }
        } catch (e: Exception) {
            Log.e(TAG, "Failed to save history: ${e.message}")
        }
    }

    private fun loadHistory() {
        if (historyLoaded) return
        val ctx = contextRef ?: return
        try {
            ctx.openFileInput(HISTORY_FILE).use { input ->
                val json = input.bufferedReader().readText()
                val arr = JSONArray(json)
                history.clear()
                for (i in 0 until arr.length()) {
                    val obj = arr.getJSONObject(i)
                    history.add(TransferRecord(
                        id = obj.getString("id"),
                        filename = obj.getString("filename"),
                        totalBytes = obj.getLong("totalBytes"),
                        direction = TransferDirection.valueOf(obj.getString("direction")),
                        peerIp = obj.optString("peerIp", ""),
                        timestamp = obj.getLong("timestamp"),
                        status = TransferStatus.valueOf(obj.getString("status")),
                        averageSpeed = obj.optDouble("averageSpeed", 0.0),
                        durationMs = obj.optLong("durationMs", 0L)
                    ))
                }
            }
        } catch (_: FileNotFoundException) {
        } catch (e: Exception) {
            Log.e(TAG, "Failed to load history: ${e.message}")
        }
        historyLoaded = true
    }

    fun resolveFileName(ctx: Context, uri: Uri): String {
        var name = "file"
        try {
            val cursor = ctx.contentResolver.query(uri, null, null, null, null)
            cursor?.use { c ->
                if (c.moveToFirst()) {
                    val idx = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                    if (idx >= 0) name = c.getString(idx) ?: "file"
                }
            }
        } catch (_: Exception) {}
        return name
    }

    fun resolveFileSize(ctx: Context, uri: Uri): Long {
        try {
            return ctx.contentResolver.openFileDescriptor(uri, "r")?.statSize ?: 0L
        } catch (_: Exception) {
            return 0L
        }
    }

    fun createTransferNotificationChannels(ctx: Context) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val manager = ctx.getSystemService(Context.NOTIFICATION_SERVICE) as android.app.NotificationManager
            val channels = listOf(
                android.app.NotificationChannel("pdos_upload_channel", "File Uploads", android.app.NotificationManager.IMPORTANCE_LOW).apply {
                    description = "Upload progress"
                },
                android.app.NotificationChannel("pdos_download_channel", "File Downloads", android.app.NotificationManager.IMPORTANCE_LOW).apply {
                    description = "Download progress"
                }
            )
            channels.forEach { manager.createNotificationChannel(it) }
        }
    }
}
