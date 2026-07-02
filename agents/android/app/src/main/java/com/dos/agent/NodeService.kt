package com.dos.agent

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import androidx.localbroadcastmanager.content.LocalBroadcastManager
import android.os.Build
import android.os.Environment
import android.os.IBinder
import androidx.core.app.NotificationCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.json.JSONObject
import java.io.File
import java.io.FileWriter
import java.net.HttpURLConnection
import java.net.URL
import javax.net.ssl.HttpsURLConnection
import javax.net.ssl.SSLContext
import javax.net.ssl.TrustManager
import javax.net.ssl.X509TrustManager
import java.security.cert.X509Certificate
import java.security.MessageDigest
import android.net.Uri

class NodeService : Service() {

    companion object {
        const val ACTION_START = "ACTION_START"
        const val ACTION_STOP = "ACTION_STOP"
        const val ACTION_ACCEPT_DOWNLOAD = "ACTION_ACCEPT_DOWNLOAD"
        const val ACTION_DECLINE_DOWNLOAD = "ACTION_DECLINE_DOWNLOAD"
        const val ACTION_SEND_TO_MAC = "ACTION_SEND_TO_MAC"
        const val ACTION_SEND_TEXT = "ACTION_SEND_TEXT"

        private const val CHANNEL_ID = "pdos_node_channel"
        private const val ALERT_CHANNEL_ID = "pdos_alert_channel"
        private const val DOWNLOAD_CHANNEL_ID = "pdos_download_channel"
        private const val DOWNLOAD_SIGNAL_PREFIX = "PDOS_DOWNLOAD::"

        private val _nodeState = MutableStateFlow("Offline")
        val nodeState = _nodeState.asStateFlow()

        private val _logs = MutableStateFlow(listOf<String>())
        val logs = _logs.asStateFlow()

        // Pending download intent: notif ID → download info
        private val pendingDownloads = mutableMapOf<Int, Quintuple<String, String, Long, String, String>>() // notifId → (url, filename, size, fingerprint, pairingCode)

        private data class Quadruple<A, B, C, D>(val first: A, val second: B, val third: C, val fourth: D)
        private data class Quintuple<A, B, C, D, E>(val first: A, val second: B, val third: C, val fourth: D, val fifth: E)
    }

    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var wakeLock: android.os.PowerManager.WakeLock? = null
    private var wifiLock: android.net.wifi.WifiManager.WifiLock? = null
    private var isAdvertising = false
    private lateinit var screenMirrorProvider: ScreenMirrorProvider
    private lateinit var cameraProvider: CameraProvider


    override fun onCreate() {
        super.onCreate()
        SettingsManager.init(applicationContext)
        FileTransferManager.init(applicationContext)
        createNotificationChannels()
        screenMirrorProvider = ScreenMirrorProvider(applicationContext)
        cameraProvider = CameraProvider(applicationContext)

        // Catch all uncaught exceptions to prevent crashes
        Thread.setDefaultUncaughtExceptionHandler { thread, throwable ->
            android.util.Log.e("NodeService", "Uncaught exception on ${thread.name}: ${throwable.message}", throwable)
            addLog("CRASH: ${throwable.message}")
        }
    }

    private fun startMdnsAdvertising() {
        if (isAdvertising) return
        isAdvertising = true
        addLog("Starting mDNS advertising (_xync._tcp)...")
        registerNsdService()
    }

    private fun stopMdnsAdvertising() {
        if (!isAdvertising) return
        isAdvertising = false
        nsdManager?.unregisterService(nsdRegistrationListener)
        addLog("mDNS advertising stopped")
    }

    private var nsdManager: NsdManager? = null
    private var nsdServiceInfo: NsdServiceInfo? = null

    private var nsdRegistrationListener = object : NsdManager.RegistrationListener {
        override fun onServiceRegistered(info: NsdServiceInfo) {
            addLog("mDNS registered: ${info.serviceName}")
        }
        override fun onRegistrationFailed(info: NsdServiceInfo, errorCode: Int) {
            addLog("mDNS registration failed: $errorCode")
            isAdvertising = false
        }
        override fun onServiceUnregistered(info: NsdServiceInfo) {
            addLog("mDNS unregistered")
            isAdvertising = false
        }
        override fun onUnregistrationFailed(info: NsdServiceInfo, errorCode: Int) {
            addLog("mDNS unregistration failed: $errorCode")
            isAdvertising = false
        }
    }

    private fun registerNsdService() {
        try {
            nsdManager = getSystemService(Context.NSD_SERVICE) as NsdManager
            val serviceInfo = NsdServiceInfo().apply {
                serviceType = "_xync._tcp"
                serviceName = "PDOS-Android-${Build.MODEL.replace(" ", "-")}"
                port = 7891
                setAttribute("node_name", Build.MODEL)
                setAttribute("platform", "android")
                setAttribute("version", "1.0.0")
            }
            nsdServiceInfo = serviceInfo
            nsdManager?.registerService(serviceInfo, NsdManager.PROTOCOL_DNS_SD, nsdRegistrationListener)
            android.util.Log.i("dos_agent", "NSD registered: ${serviceInfo.serviceName} on _xync._tcp:7891")
        } catch (e: Exception) {
            addLog("NSD registration error: ${e.message}")
            android.util.Log.e("dos_agent", "NSD registration error: ${e.message}")
            isAdvertising = false
        }
    }

    // ═══════════════════════════════════════════════════════════════
    //  mDNS DISCOVERY — browse for Mac peers on any WiFi
    // ═══════════════════════════════════════════════════════════════

    data class DiscoveredPeer(val name: String, val host: String, val port: Int, val platform: String)

    private var isDiscovering = false
    private val discoveredPeers = mutableListOf<DiscoveredPeer>()
    private var nsdDiscoveryListener: NsdManager.DiscoveryListener? = null
    private var nsdPdosListener: NsdManager.DiscoveryListener? = null
    private val tunnelClients = mutableMapOf<String, TunnelClient>()

    private fun startMdnsDiscovery() {
        if (isDiscovering) return
        isDiscovering = true
        addLog("Starting mDNS discovery (_xync._tcp & _pdos._tcp)...")
        android.util.Log.i("dos_agent", "Starting mDNS discovery...")
        try {
            nsdManager = getSystemService(Context.NSD_SERVICE) as NsdManager
            val sharedListener = createDiscoveryListener()
            nsdDiscoveryListener = sharedListener
            nsdPdosListener = createDiscoveryListener()
            nsdManager?.discoverServices("_xync._tcp", NsdManager.PROTOCOL_DNS_SD, nsdDiscoveryListener)
            nsdManager?.discoverServices("_pdos._tcp", NsdManager.PROTOCOL_DNS_SD, nsdPdosListener)
            // Also start hotspot subnet scanner as fallback (mDNS blocked on hotspot)
            // Only scan if NOT on regular WiFi (hotspot scan is for hotspot use case)
            if (!isOnWifi()) {
                startHotspotScan()
            } else {
                addLog("On WiFi — skipping hotspot scan")
            }
        } catch (e: Exception) {
            addLog("mDNS discovery error: ${e.message}")
            android.util.Log.e("dos_agent", "mDNS discovery error: ${e.message}")
            isDiscovering = false
        }
    }

    private fun startHotspotScan() {
        serviceScope.launch {
            var attempts = 0
            while (attempts < 6 && isDiscovering) {
                delay(10_000L) // wait 10s between scans
                // Only scan if we have no Mac peer yet
                val hasMac = synchronized(discoveredPeers) {
                    discoveredPeers.any { it.platform == "mac" }
                }
                if (hasMac) continue
                attempts++
                addLog("Hotspot scan attempt $attempts/6...")
                val client = TunnelClient.scanAndConnect()
                if (client != null) {
                    val ip = client.macIp
                    tunnelClients[ip] = client
                    addLog("Hotspot: found Mac at $ip, tunnel connected")
                    val peer = DiscoveredPeer("PDOS-Mac-Hotspot", ip, 7894, "mac")
                    synchronized(discoveredPeers) {
                        discoveredPeers.removeAll { it.host == ip }
                        discoveredPeers.add(peer)
                    }
                    val intent = Intent("PDOS_PEER_FOUND").apply {
                        putExtra("name", peer.name)
                        putExtra("host", peer.host)
                        putExtra("port", peer.port)
                        putExtra("platform", peer.platform)
                    }
                    try {
                        LocalBroadcastManager.getInstance(this@NodeService).sendBroadcast(intent)
                    } catch (_: Exception) {}
                    break
                }
                addLog("Hotspot scan attempt $attempts: no Mac found")
            }
        }
    }

    private fun isOnWifi(): Boolean {
        val cm = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        val network = cm.activeNetwork ?: return false
        val caps = cm.getNetworkCapabilities(network) ?: return false
        return caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)
    }

    private fun createDiscoveryListener(): NsdManager.DiscoveryListener {
        return object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(regType: String) {
                addLog("mDNS discovery started: $regType")
            }
            override fun onDiscoveryStopped(serviceType: String) {
                addLog("mDNS discovery stopped: $serviceType")
            }
            override fun onServiceFound(serviceInfo: NsdServiceInfo) {
                addLog("mDNS service found: ${serviceInfo.serviceName} (${serviceInfo.serviceType})")
                val st = serviceInfo.serviceType
                if (st == "_xync._tcp" || st == "_pdos._tcp") {
                    // Don't discover ourselves
                    if (!serviceInfo.serviceName.startsWith("PDOS-Android-")) {
                        nsdManager?.resolveService(serviceInfo, object : NsdManager.ResolveListener {
                            override fun onResolveFailed(info: NsdServiceInfo?, errorCode: Int) {
                                addLog("mDNS resolve failed for ${info?.serviceName}: $errorCode")
                            }
                            override fun onServiceResolved(info: NsdServiceInfo) {
                                val host = info.host?.hostAddress ?: return
                                val name = info.serviceName
                                val port = info.port
                                val platform = try {
                                    val attrs = info.attributes
                                    attrs?.get("platform")?.let { String(it) } ?: "mac"
                                } catch (_: Exception) { "mac" }
                                addLog("mDNS resolved: $name @ $host:$port (platform=$platform)")
                                android.util.Log.i("dos_agent", "mDNS peer: $name @ $host:$port (platform=$platform)")

                                // Connect reverse tunnel to Mac for hotspot workaround
                                if (platform == "mac") {
                                    val tunnelPort = 7895
                                    val existing = tunnelClients.remove(host)
                                    existing?.stop()
                                    val client = TunnelClient(host, tunnelPort)
                                    tunnelClients[host] = client
                                    client.connect()
                                    addLog("Reverse tunnel connecting to $host:$tunnelPort")
                                }

                                val peer = DiscoveredPeer(name, host, port, platform)
                                synchronized(discoveredPeers) {
                                    discoveredPeers.removeAll { it.host == host }
                                    discoveredPeers.add(peer)
                                }
                                val intent = Intent("PDOS_PEER_FOUND").apply {
                                    putExtra("name", name)
                                    putExtra("host", host)
                                    putExtra("port", port)
                                    putExtra("platform", platform)
                                }
                                try {
                                    LocalBroadcastManager.getInstance(this@NodeService).sendBroadcast(intent)
                                } catch (_: Exception) {}
                            }
                        })
                    }
                }
            }
            override fun onServiceLost(serviceInfo: NsdServiceInfo) {
                addLog("mDNS service lost: ${serviceInfo.serviceName}")
                val lostHost = serviceInfo.host?.hostAddress
                if (lostHost != null) {
                    synchronized(discoveredPeers) {
                        discoveredPeers.removeAll { it.host == lostHost }
                    }
                }
                val intent = Intent("PDOS_PEER_LOST").apply {
                    putExtra("name", serviceInfo.serviceName)
                }
                try {
                    LocalBroadcastManager.getInstance(this@NodeService).sendBroadcast(intent)
                } catch (_: Exception) {}
            }
            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                addLog("mDNS discovery start failed: $errorCode")
                isDiscovering = false
            }
            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {
                addLog("mDNS discovery stop failed: $errorCode")
            }
        }
    }

    private fun stopMdnsDiscovery() {
        if (!isDiscovering) return
        isDiscovering = false
        try {
            nsdDiscoveryListener?.let { nsdManager?.stopServiceDiscovery(it) }
            nsdPdosListener?.let { nsdManager?.stopServiceDiscovery(it) }
        } catch (_: Exception) {}
        addLog("mDNS discovery stopped")
    }

    fun getDiscoveredPeers(): List<DiscoveredPeer> = synchronized(discoveredPeers) {
        discoveredPeers.toList()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val notification = NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("PDOS Node")
            .setContentText("Waiting for connection on port 7891...")
            .setSmallIcon(R.drawable.ic_settings)
            .build()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            val fgsType = ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC
            @Suppress("DEPRECATION")
            startForeground(1, notification, fgsType)
        } else {
            startForeground(1, notification)
        }
        
        when (intent?.action) {
            ACTION_START -> {
                addLog("Starting PDOS P2P node...")
                acquireLocks()
                startMdnsAdvertising()
                startMdnsDiscovery()
                startP2pNode()
            }
            null -> {
                addLog("Starting PDOS P2P node (auto)...")
                acquireLocks()
                startMdnsAdvertising()
                startMdnsDiscovery()
                startP2pNode()
            }
            ACTION_STOP -> {
                releaseLocks()
                stopMdnsAdvertising()
                stopMdnsDiscovery()
                stopNode()
            }
            ACTION_ACCEPT_DOWNLOAD -> {
                val notifId = intent?.getIntExtra("NOTIF_ID", -1) ?: -1
                val data = pendingDownloads.remove(notifId)
                if (data != null) {
                    val (url, filename, size, fingerprint, pairingCode) = data
                    addLog("User accepted: downloading $filename")
                    cancelNotification(notifId)
                    val senderHost = try { java.net.URI(url).host } catch (_: Exception) { "" }
                    // Auto-trust this peer on accept
                    if (senderHost.isNotEmpty()) SettingsManager.addTrustedPeer(senderHost)
                    FileTransferManager.startDownload(url, filename, size, senderHost, SettingsManager.encryptionEnabled)
                }
            }
            ACTION_DECLINE_DOWNLOAD -> {
                val notifId = intent?.getIntExtra("NOTIF_ID", -1) ?: -1
                pendingDownloads.remove(notifId)
                cancelNotification(notifId)
                addLog("User declined file transfer")
            }
            ACTION_SEND_TO_MAC -> {
                val fileUri = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    intent?.getParcelableExtra("FILE_URI", Uri::class.java)
                } else {
                    @Suppress("DEPRECATION")
                    intent?.getParcelableExtra("FILE_URI")
                }
                val macIp = intent?.getStringExtra("MAC_IP") ?: ""
                val useEncryption = intent?.getBooleanExtra("USE_ENCRYPTION", false) ?: false
                if (fileUri != null) {
                    val filename = FileTransferManager.resolveFileName(this, fileUri)
                    val fileSize = FileTransferManager.resolveFileSize(this, fileUri)
                    FileTransferManager.startUpload(fileUri, filename, fileSize, macIp, useEncryption)
                }
            }
            ACTION_SEND_TEXT -> {
                val text = intent?.getStringExtra("TEXT_CONTENT") ?: ""
                val macIp = intent?.getStringExtra("MAC_IP") ?: ""
                val useEncryption = intent?.getBooleanExtra("USE_ENCRYPTION", false) ?: false
                if (text.isNotEmpty()) {
                    FileTransferManager.startTextTransfer(text, macIp, useEncryption)
                }
            }
        }
        return START_STICKY
    }

    private fun startP2pNode() {
        addLog("Starting P2P node on port 7891...")

        val notification = NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("PDOS Node")
            .setContentText("Listening on port 7891")
            .setSmallIcon(R.drawable.ic_settings)
            .build()

        val notificationManager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        notificationManager.notify(1, notification)
        _nodeState.value = "Listening..."

        val configPath = File(filesDir, "dos-config.toml").absolutePath
        FileWriter(File(configPath)).use { 
            it.write("""relay_url = "p2p"
node_port = 7891
node_name = "${Build.MODEL}"
""")
        }

        val clipboard = getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
        val mainHandler = android.os.Handler(android.os.Looper.getMainLooper())

        Core.startAgent(configPath, object : NodeCallback {
            override fun onStateChanged(stateJson: String) {
                _nodeState.value = stateJson
                addLog("State: $stateJson")
                if (stateJson == "Disconnected" || stateJson == "Offline") {
                    addLog("Disconnected, waiting for incoming connection...")
                }
            }
            override fun onLog(level: Int, message: String) {
                addLog("[$level] $message")
            }
            override fun getClipboard(): String {
                return try {
                    if (clipboard.hasPrimaryClip()) {
                        clipboard.primaryClip?.getItemAt(0)?.text?.toString() ?: ""
                    } else ""
                } catch (e: Exception) { "" }
            }
            override fun setClipboard(text: String) {
                mainHandler.post {
                    try {
                        clipboard.setPrimaryClip(android.content.ClipData.newPlainText("PDOS", text))
                    } catch (e: Exception) { e.printStackTrace() }
                }
            }
            override fun showNotification(title: String, body: String) {
                // Post to main thread to avoid CalledFromWrongThreadException
                android.os.Handler(android.os.Looper.getMainLooper()).post {
                    try {
                        if (body.startsWith(DOWNLOAD_SIGNAL_PREFIX)) {
                            val json = body.removePrefix(DOWNLOAD_SIGNAL_PREFIX)
                            val obj = JSONObject(json)
                            val url = obj.getString("url")
                            val filename = obj.getString("filename")
                            val size = obj.optLong("size", 0L)
                            val fingerprint = obj.optString("fingerprint", "")
                            addLog("Incoming file: $filename (${size / 1024}KB) from $url")
                            showAcceptDeclineNotification(url, filename, size, fingerprint)
                            return@post
                        }
                        // Normal notification
                        val manager = getSystemService(NotificationManager::class.java)
                        if (manager != null) {
                            val notif = NotificationCompat.Builder(this@NodeService, ALERT_CHANNEL_ID)
                                .setContentTitle(title)
                                .setContentText(body)
                                .setSmallIcon(android.R.drawable.ic_dialog_info)
                                .setPriority(NotificationCompat.PRIORITY_HIGH)
                                .setDefaults(NotificationCompat.DEFAULT_ALL)
                                .setAutoCancel(true)
                                .build()
                            manager.notify(System.currentTimeMillis().toInt(), notif)
                        }
                    } catch (e: Exception) {
                        android.util.Log.e("NodeService", "Notification error: ${e.message}")
                    }
                }
            }
        })
    }

    // Phase 1C: Show Accept / Decline before downloading
    private fun showAcceptDeclineNotification(url: String, filename: String, size: Long, fingerprint: String) {
        val senderHost = try { java.net.URI(url).host } catch (_: Exception) { null }
        val pairingCode = FileTransferManager.generatePairingCode()

        // Auto-accept check: if enabled and sender is trusted, skip the prompt
        if (SettingsManager.autoAccept && senderHost != null && SettingsManager.isTrustedPeer(senderHost)) {
            addLog("Auto-accepting from trusted peer $senderHost: $filename")
            FileTransferManager.startDownload(url, filename, size, senderHost, SettingsManager.encryptionEnabled)
            return
        }

        val notifId = System.currentTimeMillis().toInt()
        pendingDownloads[notifId] = Quintuple(url, filename, size, fingerprint, pairingCode)

        val acceptIntent = PendingIntent.getService(
            this, notifId * 2,
            Intent(this, NodeService::class.java).apply {
                action = ACTION_ACCEPT_DOWNLOAD
                putExtra("NOTIF_ID", notifId)
            },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val declineIntent = PendingIntent.getService(
            this, notifId * 2 + 1,
            Intent(this, NodeService::class.java).apply {
                action = ACTION_DECLINE_DOWNLOAD
                putExtra("NOTIF_ID", notifId)
            },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val isTrusted = senderHost != null && SettingsManager.isTrustedPeer(senderHost)
        val sizeFmt = if (size > 1_048_576) "%.1f MB".format(size / 1_048_576.0)
                      else "${size / 1024} KB"

        val title = if (isTrusted) "Incoming File (Trusted)" else "Incoming File"
        val contentText = if (isTrusted) {
            "$filename ($sizeFmt) from ${senderHost ?: "Mac"}"
        } else {
            "$filename ($sizeFmt) — Code: $pairingCode"
        }

        val notif = NotificationCompat.Builder(this, ALERT_CHANNEL_ID)
            .setContentTitle(title)
            .setContentText(contentText)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setDefaults(NotificationCompat.DEFAULT_ALL)
            .addAction(android.R.drawable.ic_input_add, "Accept & Trust", acceptIntent)
            .addAction(android.R.drawable.ic_delete, "Decline", declineIntent)
            .setAutoCancel(false)
            .setOngoing(true)
            .build()

        getSystemService(NotificationManager::class.java)?.notify(notifId, notif)
    }

    private fun cancelNotification(id: Int) {
        getSystemService(NotificationManager::class.java)?.cancel(id)
    }

    private fun stopNode() {
        Core.stopAgent()
        _nodeState.value = "Offline"
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun addLog(message: String) {
        _logs.update { current ->
            val mutable = current.toMutableList()
            mutable.add(message)
            if (mutable.size > 100) mutable.removeAt(0)
            mutable
        }
        updateQuickSettingsTile()
    }

    private fun updateQuickSettingsTile() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
            try {
                val intent = Intent(this, PdosTileService::class.java)
                sendBroadcast(intent)
            } catch (_: Exception) {}
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        super.onDestroy()
        addLog("NodeService onDestroy")
        releaseLocks()
        serviceScope.cancel()
        Core.stopAgent()
        screenMirrorProvider.stop()
        cameraProvider.stop()
        stopMdnsAdvertising()
        stopMdnsDiscovery()
        try { FileTransferManager.cleanup() } catch (_: Exception) {}
        tunnelClients.values.forEach { it.stop() }
        tunnelClients.clear()
        _nodeState.value = "Offline"
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        super.onTaskRemoved(rootIntent)
        addLog("NodeService task removed, re-showing notification")
        // Re-show foreground notification so user knows service is still running
        try {
            val notification = NotificationCompat.Builder(this, CHANNEL_ID)
                .setContentTitle("PDOS Active")
                .setContentText("Service running in background")
                .setSmallIcon(R.drawable.ic_settings)
                .build()
            startForeground(1, notification)
        } catch (_: Exception) {}
    }

    private fun createNotificationChannels() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val manager = getSystemService(NotificationManager::class.java)
            manager?.createNotificationChannel(
                NotificationChannel(CHANNEL_ID, "PDOS Node Service", NotificationManager.IMPORTANCE_LOW)
            )
            manager?.createNotificationChannel(
                NotificationChannel(ALERT_CHANNEL_ID, "PDOS Alerts", NotificationManager.IMPORTANCE_HIGH).apply {
                    enableLights(true)
                    enableVibration(true)
                    lockscreenVisibility = android.app.Notification.VISIBILITY_PUBLIC
                }
            )
            manager?.createNotificationChannel(
                NotificationChannel(DOWNLOAD_CHANNEL_ID, "PDOS File Transfers", NotificationManager.IMPORTANCE_LOW).apply {
                    description = "File transfer progress"
                }
            )
        }
        FileTransferManager.createTransferNotificationChannels(this)
    }

    private fun acquireLocks() {
        try {
            val pm = getSystemService(Context.POWER_SERVICE) as android.os.PowerManager
            if (wakeLock == null) {
                wakeLock = pm.newWakeLock(android.os.PowerManager.PARTIAL_WAKE_LOCK, "PDOS::NodeServiceWakeLock").apply {
                    acquire()
                }
                addLog("WakeLock acquired")
            }
            val wm = getSystemService(Context.WIFI_SERVICE) as android.net.wifi.WifiManager
            if (wifiLock == null) {
                @Suppress("DEPRECATION")
                wifiLock = wm.createWifiLock(android.net.wifi.WifiManager.WIFI_MODE_FULL_HIGH_PERF, "PDOS::NodeServiceWifiLock").apply {
                    acquire()
                }
                addLog("WifiLock acquired")
            }
        } catch (e: Exception) {
            addLog("Error acquiring locks: ${e.message}")
        }
    }

    private fun releaseLocks() {
        try {
            wakeLock?.let {
                if (it.isHeld) it.release()
                addLog("WakeLock released")
            }
            wakeLock = null
            wifiLock?.let {
                if (it.isHeld) it.release()
                addLog("WifiLock released")
            }
            wifiLock = null
        } catch (e: Exception) {
            addLog("Error releasing locks: ${e.message}")
        }
    }
}
