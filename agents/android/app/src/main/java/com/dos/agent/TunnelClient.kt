package com.dos.agent

import android.util.Log
import kotlinx.coroutines.*
import java.io.*
import java.net.Socket
import java.net.InetSocketAddress



/**
 * Connects to the Mac's reverse tunnel port (7895) and receives files.
 * The phone initiates this connection (outbound from hotspot = allowed),
 * then the Mac pushes file data through it.
 *
 * Frame format: [4-byte filename_len][filename][8-byte file_size][file_data]
 */
class TunnelClient(val macIp: String, private val tunnelPort: Int = 7895) {

    companion object {
        /**
         * Probe the hotspot subnet for a Mac running the reverse tunnel.
         * Tries common hotspot client IPs (gateway .1 + /24 scan of last octet).
         */
        fun scanAndConnect(tunnelPort: Int = 7895): TunnelClient? {
            try {
                // Find our hotspot interface IP
                val interfaces = java.net.NetworkInterface.getNetworkInterfaces()
                while (interfaces.hasMoreElements()) {
                    val iface = interfaces.nextElement()
                    if (!iface.isUp || iface.isLoopback) continue
                    val name = iface.name ?: continue
                    // Hotspot interfaces on Samsung: swlan0, ap0, wlan0
                    if (!name.startsWith("swlan") && !name.startsWith("ap") && name != "wlan0") continue

                    for (addr in iface.inetAddresses) {
                        val hostAddr = addr.hostAddress ?: continue
                        if (!hostAddr.contains('.')) continue
                        // This is our hotspot IP, e.g. 10.233.89.189
                        val prefix = hostAddr.substringBeforeLast('.')
                        Log.i("dos_agent", "Tunnel scan: hotspot subnet $prefix.0/24")

                        // Try common client IPs first (faster)
                        val candidates = listOf(
                            "$prefix.2", "$prefix.3", "$prefix.4", "$prefix.5",
                            "$prefix.100", "$prefix.101", "$prefix.102",
                            "$prefix.128", "$prefix.129", "$prefix.130",
                            "$prefix.254", "$prefix.253", "$prefix.252",
                        ) + (2..254).map { "$prefix.$it" }

                        for (ip in candidates) {
                            try {
                                val s = Socket()
                                s.connect(java.net.InetSocketAddress(ip, tunnelPort), 300)
                                s.close()
                                Log.i("dos_agent", "Tunnel scan: found Mac at $ip:$tunnelPort")
                                return TunnelClient(ip, tunnelPort)
                            } catch (_: Exception) {
                                // No response, try next
                            }
                        }
                    }
                }
            } catch (e: Exception) {
                Log.e("dos_agent", "Tunnel scan error: ${e.message}")
            }
            return null
        }

        /**
         * Send a file to Mac via tunnel (one-shot: connect, send, disconnect).
         * Used when there's no existing tunnel connection.
         */
        fun sendFileToMac(macIp: String, file: java.io.File, filename: String, tunnelPort: Int = 7896, progressCallback: ((Long) -> Unit)? = null): Boolean {
            return try {
                val s = java.net.Socket()
                s.connect(java.net.InetSocketAddress(macIp, tunnelPort), 5000)
                s.soTimeout = 300_000

                val fileSize = file.length()
                val nameBytes = filename.toByteArray(Charsets.UTF_8)
                val out = java.io.DataOutputStream(java.io.BufferedOutputStream(s.getOutputStream()))

                // Write frame header: [4-byte name_len][name][8-byte file_size]
                out.writeInt(nameBytes.size)
                out.write(nameBytes)
                out.writeLong(fileSize)

                // Write file data
                val buffer = ByteArray(65536)
                val fis = java.io.FileInputStream(file)
                var totalSent = 0L
                try {
                    var bytesRead: Int
                    while (fis.read(buffer).also { bytesRead = it } != -1) {
                        out.write(buffer, 0, bytesRead)
                        totalSent += bytesRead
                        progressCallback?.invoke(totalSent)
                    }
                    out.flush()
                } finally {
                    fis.close()
                }

                s.close()
                Log.i("dos_agent", "Tunnel: sent $filename ($fileSize bytes) to Mac via one-shot connection")
                true
            } catch (e: Exception) {
                Log.e("dos_agent", "Tunnel one-shot send error: ${e.message}")
                false
            }
        }
    }

    private var socket: Socket? = null
    private var input: DataInputStream? = null
    private var job: Job? = null
    private val scope = CoroutineScope(Dispatchers.IO + SupervisorJob())

    fun connect() {
        job = scope.launch {
            var retries = 0
            while (retries < 5 && isActive) {
                try {
                    Log.i("dos_agent", "Tunnel: connecting to $macIp:$tunnelPort...")
                    val s = Socket()
                    s.connect(java.net.InetSocketAddress(macIp, tunnelPort), 5000)
                    socket = s
                    input = DataInputStream(BufferedInputStream(s.getInputStream()))
                    Log.i("dos_agent", "Tunnel: connected to $macIp:$tunnelPort")
                    readLoop()
                    break
                } catch (e: Exception) {
                    Log.w("dos_agent", "Tunnel: connection failed (attempt ${retries + 1}): ${e.message}")
                    retries++
                    delay(5000)
                }
            }
            if (retries >= 5) {
                Log.e("dos_agent", "Tunnel: giving up after 5 retries to $macIp")
            }
        }
    }

    private fun readLoop() {
        try {
            val dis = input ?: return
            while (true) {
                // Read 4-byte filename length
                val nameLen = dis.readInt()
                if (nameLen <= 0 || nameLen > 1024) {
                    Log.w("dos_agent", "Tunnel: invalid filename length: $nameLen")
                    break
                }

                // Read filename
                val nameBytes = ByteArray(nameLen)
                dis.readFully(nameBytes)
                val filename = String(nameBytes, Charsets.UTF_8)

                // Read 8-byte file size
                val fileSize = dis.readLong()
                if (fileSize <= 0 || fileSize > 10L * 1024 * 1024 * 1024) {
                    Log.w("dos_agent", "Tunnel: invalid file size: $fileSize")
                    break
                }

                Log.i("dos_agent", "Tunnel: receiving $filename ($fileSize bytes)")

                // Read file data
                val downloadDir = SettingsManager.getDownloadDir()
                val outputFile = File(downloadDir, filename)
                outputFile.parentFile?.mkdirs()

                val fos = FileOutputStream(outputFile)
                val buf = ByteArray(65536)
                var remaining = fileSize
                while (remaining > 0) {
                    val read = dis.read(buf, 0, minOf(buf.size.toLong(), remaining).toInt())
                    if (read < 0) throw EOFException("Unexpected EOF")
                    fos.write(buf, 0, read)
                    remaining -= read
                }
                fos.close()

                Log.i("dos_agent", "Tunnel: saved $filename to ${outputFile.absolutePath}")
            }
        } catch (e: EOFException) {
            Log.i("dos_agent", "Tunnel: Mac closed connection")
        } catch (e: Exception) {
            Log.e("dos_agent", "Tunnel: read error: ${e.message}")
        } finally {
            disconnect()
        }
    }

    /**
     * Send a file to Mac through the existing tunnel connection.
     * Frame format matches Mac tunnel server: [4-byte name_len][name][8-byte file_size][file_data]
     */
    fun sendFile(file: java.io.File, filename: String, progressCallback: ((Long) -> Unit)? = null): Boolean {
        val s = socket ?: return false
        try {
            val fileSize = file.length()
            val nameBytes = filename.toByteArray(Charsets.UTF_8)
            val out = java.io.DataOutputStream(java.io.BufferedOutputStream(s.getOutputStream()))

            // Write frame header: [4-byte name_len][name][8-byte file_size]
            out.writeInt(nameBytes.size)
            out.write(nameBytes)
            out.writeLong(fileSize)

            // Write file data in 64KB chunks
            val buffer = ByteArray(65536)
            val fis = java.io.FileInputStream(file)
            var totalSent = 0L
            try {
                var bytesRead: Int
                while (fis.read(buffer).also { bytesRead = it } != -1) {
                    out.write(buffer, 0, bytesRead)
                    totalSent += bytesRead
                    progressCallback?.invoke(totalSent)
                }
                out.flush()
            } finally {
                fis.close()
            }

            Log.i("dos_agent", "Tunnel: sent $filename ($fileSize bytes) to Mac")
            return true
        } catch (e: Exception) {
            Log.e("dos_agent", "Tunnel send error: ${e.message}")
            return false
        }
    }

    fun disconnect() {
        try { socket?.close() } catch (_: Exception) {}
        socket = null
        input = null
    }

    fun stop() {
        job?.cancel()
        disconnect()
    }
}
