package com.dos.agent

import android.os.Build
import android.util.Log
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.InputStream
import java.io.OutputStream
import java.net.Inet6Address
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketAddress
import java.net.URLDecoder
import java.net.URLEncoder
import java.nio.ByteBuffer
import java.nio.channels.FileChannel
import java.nio.channels.ServerSocketChannel
import java.nio.channels.SocketChannel
import java.nio.file.StandardOpenOption
import java.nio.charset.StandardCharsets
import java.security.MessageDigest
import java.security.cert.X509Certificate
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicLong
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream
import javax.net.ssl.*

data class TransferResult(
    val success: Boolean,
    val bytesTransferred: Long,
    val checksum: String = "",
    val errorMessage: String = ""
)

object NioTransfer {
    private const val TAG = "NioTransfer"

    const val SOCKET_BUFFER_SIZE = 4_194_304 // 4MB socket buffers
    const val DIRECT_BUFFER_SIZE = 4_194_304 // 4MB direct ByteBuffer for headers
    const val TRANSFER_CHUNK_SIZE = 4_194_304 // 4MB transferTo chunk
    const val FILE_SERVER_PORT = 7894

    // Reusable direct buffer pool to avoid allocateDirect churn
    private var sharedHeaderBuf: ByteBuffer? = null
    private const val MAX_THREADS = 8
    private const val MAX_FRAME_SIZE = 1_048_576 // 1MB max per frame
    private const val MAX_HEADER_SIZE = 65_536 // 64KB max HTTP header
    private const val MAX_BODY_SIZE = 1_048_576 * 100 // 100MB max body allocation

    // Frame types for raw TCP protocol
    private const val FRAME_METADATA = 0x01
    private const val FRAME_DATA = 0x02
    private const val FRAME_END = 0x03
    private const val FRAME_ERROR = 0xFF

    // ═══════════════════════════════════════════════════
    //  HTTP UPLOAD (Android → Mac) — NIO + zero-copy
    // ═══════════════════════════════════════════════════

    fun httpUpload(
        file: File,
        filename: String,
        host: String,
        port: Int = 8080,
        path: String = "/api/receive-file",
        checksum: String = "",
        useTls: Boolean = false,
        maxBytesPerSecond: Long = 0L,
        progressCallback: ((Long) -> Unit)? = null
    ): TransferResult {
        val fileSize = file.length()
        if (fileSize == 0L) return TransferResult(false, 0, "", "Empty file")

        if (useTls) {
            return tlsHttpUpload(file, filename, host, port, path, checksum, progressCallback)
        }

        val fileChannel = FileChannel.open(file.toPath())
        SocketChannel.open().use { channel ->
            channel.socket().apply {
                setSendBufferSize(SOCKET_BUFFER_SIZE)
                setReceiveBufferSize(SOCKET_BUFFER_SIZE)
                setTcpNoDelay(true)
                soTimeout = 300_000
            }
            channel.connect(resolveSocketAddress(host, port))

            val encodedFilename = try { URLEncoder.encode(filename, "UTF-8") } catch (_: Exception) { filename }
            val header = buildString {
                append("POST $path HTTP/1.1\r\n")
                append("Host: $host:$port\r\n")
                append("Content-Type: application/octet-stream\r\n")
                append("X-Filename: $encodedFilename\r\n")
                append("Content-Length: $fileSize\r\n")
                if (checksum.isNotEmpty()) append("X-File-Checksum: $checksum\r\n")
                append("Connection: close\r\n")
                append("\r\n")
            }
            val headerBuf = ByteBuffer.wrap(header.toByteArray(StandardCharsets.US_ASCII))
            while (headerBuf.hasRemaining()) channel.write(headerBuf)

            // Zero-copy send: FileChannel → SocketChannel with optional throttle
            var position = 0L
            var totalSent = 0L
            var throttleStart = System.currentTimeMillis()
            var throttleBytes = 0L
            while (position < fileSize) {
                throttleStart = System.currentTimeMillis()
                throttleBytes = 0L
                val count = fileChannel.transferTo(position, TRANSFER_CHUNK_SIZE.toLong(), channel)
                if (count <= 0) throw java.io.IOException("transferTo returned $count")
                position += count
                totalSent += count
                throttleBytes += count
                applyThrottle(throttleBytes, throttleStart, maxBytesPerSecond)
                progressCallback?.invoke(totalSent)
            }
            fileChannel.close()

            // Read HTTP response (small)
            val respBuf = ByteBuffer.allocate(4096)
            var responseStr = ""
            while (channel.read(respBuf) > 0) {
                respBuf.flip()
                responseStr += StandardCharsets.US_ASCII.decode(respBuf).toString()
                respBuf.clear()
            }

            val statusCode = Regex("HTTP/1\\.[01] (\\d{3})").find(responseStr)?.groupValues?.get(1)?.toIntOrNull() ?: 0
            val statusOk = statusCode in 200..299
            return TransferResult(statusOk, totalSent, "",
                if (statusOk) "" else "Server returned $statusCode: ${responseStr.take(100)}")
        }
    }

    private fun tlsHttpUpload(
        file: File,
        filename: String,
        host: String,
        port: Int,
        path: String,
        checksum: String = "",
        progressCallback: ((Long) -> Unit)? = null
    ): TransferResult {
        val fileSize = file.length()
        if (fileSize == 0L) return TransferResult(false, 0, "", "Empty file")

        try {
            val sslContext = SSLContext.getInstance("TLS")
            sslContext.init(null, trustAllCerts, java.security.SecureRandom())
            val sslFactory = sslContext.socketFactory
            val addr = resolveSocketAddress(host, port)
            val socket = sslFactory.createSocket(addr.address, addr.port) as SSLSocket
            socket.soTimeout = 300_000
            socket.startHandshake()

            val encodedFilename = try { URLEncoder.encode(filename, "UTF-8") } catch (_: Exception) { filename }
            val os = socket.getOutputStream()
            val header = buildString {
                append("POST $path HTTP/1.1\r\n")
                append("Host: $host:$port\r\n")
                append("Content-Type: application/octet-stream\r\n")
                append("X-Filename: $encodedFilename\r\n")
                append("Content-Length: $fileSize\r\n")
                if (checksum.isNotEmpty()) append("X-File-Checksum: $checksum\r\n")
                append("Connection: close\r\n")
                append("\r\n")
            }
            os.write(header.toByteArray(StandardCharsets.US_ASCII))
            os.flush()

            val buf = ByteArray(1_048_576)
            var totalSent = 0L
            FileChannel.open(file.toPath()).use { fc ->
                var position = 0L
                while (position < fileSize) {
                    val read = fc.read(ByteBuffer.wrap(buf))
                    if (read <= 0) break
                    os.write(buf, 0, read)
                    position += read
                    totalSent += read
                    progressCallback?.invoke(totalSent)
                }
            }
            os.flush()
            socket.shutdownOutput()

            val inputStream = socket.inputStream
            val respBuf = ByteArray(4096)
            val respLen = inputStream.read(respBuf)
            val responseStr = if (respLen > 0) String(respBuf, 0, respLen, StandardCharsets.US_ASCII) else ""

            socket.close()
            val statusCode = Regex("HTTP/1\\.[01] (\\d{3})").find(responseStr)?.groupValues?.get(1)?.toIntOrNull() ?: 0
            val statusOk = statusCode in 200..299
            return TransferResult(statusOk, totalSent, "",
                if (statusOk) "" else "Server returned $statusCode: ${responseStr.take(100)}")
        } catch (e: Exception) {
            return TransferResult(false, 0, "", "TLS upload error: ${e.message}")
        }
    }

    // ═══════════════════════════════════════════════════
    //  HTTP DOWNLOAD (Mac → Android) — NIO + zero-copy
    // ═══════════════════════════════════════════════════

    fun httpDownload(
        url: String,
        outputFile: File,
        expectedChecksum: String = "",
        progressCallback: ((Long) -> Unit)? = null
    ): TransferResult {
        // Delegate HTTPS to the SSL fallback path
        if (url.startsWith("https://")) {
            return httpsDownload(url, outputFile, expectedChecksum, progressCallback)
        }

        val uri = java.net.URI(url)
        val host = uri.host ?: return TransferResult(false, 0, "", "Invalid URL: $url")
        val port = uri.port.takeIf { it > 0 } ?: 80
        val path = uri.rawPath + (uri.rawQuery?.let { "?$it" } ?: "")

        val fileSize = AtomicLong(0)
        val sha256 = if (expectedChecksum.isNotEmpty()) MessageDigest.getInstance("SHA-256") else null
        outputFile.parentFile?.mkdirs()
        val fileChannel = FileChannel.open(outputFile.toPath(),
            java.nio.file.StandardOpenOption.WRITE,
            java.nio.file.StandardOpenOption.CREATE,
            java.nio.file.StandardOpenOption.TRUNCATE_EXISTING)

        SocketChannel.open().use { channel ->
            channel.socket().apply {
                setReceiveBufferSize(SOCKET_BUFFER_SIZE)
                setSendBufferSize(SOCKET_BUFFER_SIZE)
                setTcpNoDelay(true)
                soTimeout = 300_000
            }
            channel.connect(resolveSocketAddress(host, port))

            val request = "GET $path HTTP/1.1\r\nHost: $host\r\nConnection: close\r\n\r\n"
            val reqBuf = ByteBuffer.wrap(request.toByteArray(StandardCharsets.US_ASCII))
            while (reqBuf.hasRemaining()) channel.write(reqBuf)

            // Read response headers using direct buffer
            val headerBuf = ByteBuffer.allocateDirect(8192)
            var headersComplete = false
            var headerData = byteArrayOf()
            var contentLength = -1L

            while (!headersComplete) {
                headerBuf.clear()
                val bytesRead = channel.read(headerBuf)
                if (bytesRead <= 0) throw java.io.IOException("Server closed while reading headers")
                headerBuf.flip()
                val chunk = ByteArray(headerBuf.remaining())
                headerBuf.get(chunk)
                headerData += chunk

                val headerStr = String(headerData, StandardCharsets.US_ASCII)
                val endIdx = headerStr.indexOf("\r\n\r\n")
                if (endIdx >= 0) {
                    headersComplete = true
                    // Parse content-length
                    val clMatch = Regex("Content-Length:\\s*(\\d+)", RegexOption.IGNORE_CASE).find(headerStr)
                    contentLength = clMatch?.groupValues?.get(1)?.toLongOrNull() ?: -1L

                    // Calculate remaining bytes after headers
                    val bodyStart = endIdx + 4
                    val leftover = headerData.size - bodyStart
                    if (leftover > 0) {
                        val leftoverBytes = headerData.copyOfRange(bodyStart, headerData.size)
                        if (leftoverBytes.isNotEmpty()) {
                            val pos = fileChannel.position()
                            val bb = ByteBuffer.wrap(leftoverBytes)
                            while (bb.hasRemaining()) fileChannel.write(bb)
                            fileSize.addAndGet(leftover.toLong())
                            sha256?.update(leftoverBytes)
                            progressCallback?.invoke(fileSize.get())
                        }
                    }
                }
            }

            // Zero-copy receive: SocketChannel → FileChannel via transferFrom
            var totalRead = fileSize.get()
            while (true) {
                val count = fileChannel.transferFrom(channel, totalRead, TRANSFER_CHUNK_SIZE.toLong())
                if (count <= 0) break
                totalRead += count
                fileSize.addAndGet(count)
                progressCallback?.invoke(totalRead)
            }
            fileChannel.close()

            val digest = sha256?.digest()?.joinToString("") { "%02x".format(it) } ?: ""
            val checksumOk = expectedChecksum.isEmpty() || digest == expectedChecksum
            if (expectedChecksum.isNotEmpty() && !checksumOk) {
                outputFile.delete()
                return TransferResult(false, totalRead, digest, "Checksum mismatch: expected $expectedChecksum, got $digest")
            }
            return TransferResult(true, totalRead, digest)
        }
    }

    // ═══════════════════════════════════════════════════
    //  HTTPS DOWNLOAD (TLS fallback — uses SSLSocket, no zero-copy)
    // ═══════════════════════════════════════════════════

    private val trustAllCerts = arrayOf<TrustManager>(object : X509TrustManager {
        override fun checkClientTrusted(chain: Array<X509Certificate>, authType: String) {}
        override fun checkServerTrusted(chain: Array<X509Certificate>, authType: String) {}
        override fun getAcceptedIssuers(): Array<X509Certificate> = arrayOf()
    })

    private fun httpsDownload(
        url: String,
        outputFile: File,
        expectedChecksum: String = "",
        progressCallback: ((Long) -> Unit)? = null
    ): TransferResult {
        val uri = java.net.URI(url)
        val host = uri.host ?: return TransferResult(false, 0, "", "Invalid URL: $url")
        val port = uri.port.takeIf { it > 0 } ?: 443
        val path = uri.rawPath + (uri.rawQuery?.let { "?$it" } ?: "")

        outputFile.parentFile?.mkdirs()
        val sha256 = if (expectedChecksum.isNotEmpty()) MessageDigest.getInstance("SHA-256") else null

        val sslContext = SSLContext.getInstance("TLS")
        sslContext.init(null, trustAllCerts, java.security.SecureRandom())
        val sslFactory = sslContext.socketFactory
        val addr = resolveSocketAddress(host, port)
        val socket = sslFactory.createSocket(addr.address, addr.port) as SSLSocket
        socket.soTimeout = 300_000
        socket.startHandshake()

        val os = socket.getOutputStream()
        os.write("GET $path HTTP/1.1\r\nHost: $host\r\nConnection: close\r\n\r\n".toByteArray(StandardCharsets.US_ASCII))
        os.flush()

        val inputStream = socket.inputStream
        val headerBuf = ByteArray(16384)
        var headerLen = 0
        while (headerLen < headerBuf.size) {
            val b = inputStream.read()
            if (b < 0) break
            headerBuf[headerLen++] = b.toByte()
            if (headerLen >= 4 && headerBuf[headerLen - 1] == '\n'.toByte() && headerBuf[headerLen - 2] == '\r'.toByte()
                && headerBuf[headerLen - 3] == '\n'.toByte() && headerBuf[headerLen - 4] == '\r'.toByte()) {
                break
            }
        }

        // Check for response status
        val headerStr = String(headerBuf, 0, headerLen, StandardCharsets.US_ASCII)
        val statusCode = Regex("HTTP/1\\.[01] (\\d{3})").find(headerStr)?.groupValues?.get(1)?.toIntOrNull() ?: 0
        if (statusCode !in 200..299) {
            socket.close()
            return TransferResult(false, 0, "", "Server returned $statusCode: ${headerStr.take(100)}")
        }

        var totalRead = 0L
        outputFile.outputStream().use { fos ->
            val buffer = ByteArray(1_048_576)
            var bytesRead: Int
            while (inputStream.read(buffer).also { bytesRead = it } != -1) {
                fos.write(buffer, 0, bytesRead)
                sha256?.update(buffer, 0, bytesRead)
                totalRead += bytesRead
                progressCallback?.invoke(totalRead)
            }
        }
        socket.close()

        val digest = sha256?.digest()?.joinToString("") { "%02x".format(it) } ?: ""
        if (expectedChecksum.isNotEmpty() && digest != expectedChecksum) {
            outputFile.delete()
            return TransferResult(false, totalRead, digest, "Checksum mismatch")
        }
        return TransferResult(true, totalRead, digest)
    }

    // ═══════════════════════════════════════════════════
    //  RAW TCP FILE TRANSFER (peer-to-peer)
    // ═══════════════════════════════════════════════════

    fun startFileServer(outputDir: File): ServerSocketChannel {
        val serverChannel = ServerSocketChannel.open()
        serverChannel.socket().setReuseAddress(true)
        val addr = InetAddress.getByName("0.0.0.0")
        serverChannel.socket().bind(InetSocketAddress(addr, FILE_SERVER_PORT))
        Log.i(TAG, "File server listening on port $FILE_SERVER_PORT")

        val threadPool = Executors.newFixedThreadPool(MAX_THREADS)

        Thread({
            while (!Thread.currentThread().isInterrupted) {
                try {
                    val client = serverChannel.accept()
                    threadPool.submit {
                        handleFileClient(client, outputDir)
                    }
                } catch (e: java.nio.channels.ClosedByInterruptException) {
                    break
                } catch (e: Exception) {
                    Log.e(TAG, "Accept error: ${e.message}")
                }
            }
        }, "FileServer-Accept").start()

        return serverChannel
    }

    fun rawSendFile(
        host: String,
        port: Int = FILE_SERVER_PORT,
        file: File,
        filename: String,
        checksum: String = "",
        progressCallback: ((Long) -> Unit)? = null
    ): TransferResult {
        val fileSize = file.length()
        if (fileSize == 0L) return TransferResult(false, 0, "", "Empty file")

        SocketChannel.open().use { channel ->
            channel.socket().apply {
                setSendBufferSize(SOCKET_BUFFER_SIZE)
                setReceiveBufferSize(SOCKET_BUFFER_SIZE)
                setTcpNoDelay(true)
                soTimeout = 300_000
            }
            channel.connect(resolveSocketAddress(host, port))

            // Send metadata frame
            val meta = """{"filename":"$filename","totalSize":$fileSize,"checksum":"$checksum"}"""
            val metaBytes = meta.toByteArray(StandardCharsets.UTF_8)
            sendFrame(channel, FRAME_METADATA, ByteBuffer.wrap(metaBytes))

            // Send file data using zero-copy transferTo
            val fileChannel = FileChannel.open(file.toPath())
            var position = 0L
            var totalSent = 0L
            val dataBuf = ByteBuffer.allocateDirect(DIRECT_BUFFER_SIZE)

            while (position < fileSize) {
                dataBuf.clear()
                dataBuf.limit(minOf(dataBuf.capacity(), (fileSize - position).toInt()))
                val bytesRead = fileChannel.read(dataBuf)
                if (bytesRead <= 0) break
                dataBuf.flip()
                sendFrame(channel, FRAME_DATA, dataBuf)
                position += bytesRead
                totalSent += bytesRead
                progressCallback?.invoke(totalSent)
            }
            fileChannel.close()

            // Send end frame
            sendFrame(channel, FRAME_END, ByteBuffer.allocate(0))

            // Read response
            val respBuf = ByteBuffer.allocate(1024)
            channel.read(respBuf)
            respBuf.flip()
            val respStr = StandardCharsets.US_ASCII.decode(respBuf).toString()

            return TransferResult(true, totalSent, checksum)
        }
    }

    fun rawReceiveFile(
        host: String,
        port: Int = FILE_SERVER_PORT,
        outputFile: File,
        expectedChecksum: String = "",
        progressCallback: ((Long) -> Unit)? = null
    ): TransferResult {
        outputFile.parentFile?.mkdirs()
        val sha256 = if (expectedChecksum.isNotEmpty()) MessageDigest.getInstance("SHA-256") else null
        val fileChannel = FileChannel.open(outputFile.toPath(),
            java.nio.file.StandardOpenOption.WRITE,
            java.nio.file.StandardOpenOption.CREATE,
            java.nio.file.StandardOpenOption.TRUNCATE_EXISTING)

        SocketChannel.open().use { channel ->
            channel.socket().apply {
                setReceiveBufferSize(SOCKET_BUFFER_SIZE)
                setSendBufferSize(SOCKET_BUFFER_SIZE)
                setTcpNoDelay(true)
                soTimeout = 300_000
            }
            channel.connect(resolveSocketAddress(host, port))

            var totalReceived = 0L
            var receivedFilename = ""

            while (true) {
                val frame = readFrame(channel) ?: break
                when (frame.type) {
                    FRAME_METADATA -> {
                        val meta = String(frame.data, StandardCharsets.UTF_8)
                        receivedFilename = meta
                    }
                    FRAME_DATA -> {
                        val pos = fileChannel.position()
                        val bb = ByteBuffer.wrap(frame.data)
                        while (bb.hasRemaining()) fileChannel.write(bb)
                        totalReceived += frame.data.size
                        sha256?.update(frame.data)
                        progressCallback?.invoke(totalReceived)
                    }
                    FRAME_END -> {
                        break
                    }
                    FRAME_ERROR -> {
                        val errMsg = String(frame.data, StandardCharsets.UTF_8)
                        fileChannel.close()
                        outputFile.delete()
                        return TransferResult(false, totalReceived, "", errMsg)
                    }
                }
            }
            fileChannel.close()

            val digest = sha256?.digest()?.joinToString("") { "%02x".format(it) } ?: ""
            val checksumOk = expectedChecksum.isEmpty() || digest == expectedChecksum
            if (expectedChecksum.isNotEmpty() && !checksumOk) {
                outputFile.delete()
                return TransferResult(false, totalReceived, digest, "Checksum mismatch")
            }
            return TransferResult(true, totalReceived, digest)
        }
    }

    // ═══════════════════════════════════════════════════
    //  PARALLEL HTTP DOWNLOAD (Range requests)
    // ═══════════════════════════════════════════════════

    fun parallelHttpDownload(
        url: String,
        outputFile: File,
        numStreams: Int = 4,
        expectedChecksum: String = "",
        progressCallback: ((Long) -> Unit)? = null
    ): TransferResult {
        val uri = java.net.URI(url)
        val host = uri.host ?: return TransferResult(false, 0, "", "Invalid URL")
        val port = uri.port.takeIf { it > 0 } ?: 80
        val path = uri.rawPath + (uri.rawQuery?.let { "?$it" } ?: "")

        // First HEAD request to get file size
        val totalSize = getContentLength(host, port, path) ?: return TransferResult(
            false, 0, "", "Cannot determine file size"
        )
        if (totalSize <= 0) return TransferResult(false, 0, "", "Empty file")

        outputFile.parentFile?.mkdirs()
        // Pre-allocate file
        val fileChannel = FileChannel.open(outputFile.toPath(),
            java.nio.file.StandardOpenOption.WRITE,
            java.nio.file.StandardOpenOption.CREATE,
            java.nio.file.StandardOpenOption.TRUNCATE_EXISTING)
        fileChannel.position(totalSize - 1)
        fileChannel.write(ByteBuffer.wrap(byteArrayOf(0)))
        fileChannel.close()

        val totalDownloaded = AtomicLong(0)
        val chunkSize = (totalSize + numStreams - 1) / numStreams
        val errors = java.util.concurrent.ConcurrentLinkedQueue<String>()

        val threads = (0 until numStreams).map { streamIdx ->
            Thread {
                try {
                    val rangeStart = streamIdx * chunkSize
                    val rangeEnd = minOf(rangeStart + chunkSize - 1, totalSize - 1)
                    if (rangeStart >= totalSize) return@Thread

                    SocketChannel.open().use { ch ->
                        ch.socket().apply {
                            setReceiveBufferSize(SOCKET_BUFFER_SIZE)
                            setSendBufferSize(SOCKET_BUFFER_SIZE)
                            setTcpNoDelay(true)
                            soTimeout = 300_000
                        }
                ch.connect(resolveSocketAddress(host, port))

                        val req = "GET $path HTTP/1.1\r\nHost: $host\r\nRange: bytes=$rangeStart-$rangeEnd\r\nConnection: close\r\n\r\n"
                        val reqBuf = ByteBuffer.wrap(req.toByteArray(StandardCharsets.US_ASCII))
                        while (reqBuf.hasRemaining()) ch.write(reqBuf)

                        // Read headers
                        val headerBuf = ByteBuffer.allocateDirect(8192)
                        var headerBytes = byteArrayOf()
                        while (true) {
                            headerBuf.clear()
                            val n = ch.read(headerBuf)
                            if (n <= 0) throw java.io.IOException("Server closed")
                            headerBuf.flip()
                            val chunk = ByteArray(headerBuf.remaining())
                            headerBuf.get(chunk)
                            headerBytes += chunk
                            if (String(headerBytes, StandardCharsets.US_ASCII).contains("\r\n\r\n")) break
                        }

                        val bodyStart = String(headerBytes, StandardCharsets.US_ASCII).indexOf("\r\n\r\n") + 4
                        val leftover = headerBytes.copyOfRange(bodyStart, headerBytes.size)

                        // Write leftover bytes
                        if (leftover.isNotEmpty()) {
                            val fc = FileChannel.open(outputFile.toPath(),
                                java.nio.file.StandardOpenOption.WRITE)
                            fc.position(rangeStart)
                            fc.write(ByteBuffer.wrap(leftover))
                            fc.close()
                            totalDownloaded.addAndGet(leftover.size.toLong())
                            progressCallback?.invoke(totalDownloaded.get())
                        }

                        // transferFrom remaining
                        val fc2 = FileChannel.open(outputFile.toPath(),
                            java.nio.file.StandardOpenOption.WRITE)
                        var pos = (rangeStart + leftover.size).toLong()
                        while (true) {
                            val cnt = fc2.transferFrom(ch, pos, TRANSFER_CHUNK_SIZE.toLong())
                            if (cnt <= 0) break
                            pos += cnt
                            totalDownloaded.addAndGet(cnt)
                            progressCallback?.invoke(totalDownloaded.get())
                        }
                        fc2.close()
                    }
                } catch (e: Exception) {
                    errors.add("Stream $streamIdx: ${e.message}")
                }
            }
        }

        threads.forEach { it.start() }
        threads.forEach { it.join(120_000) }

        val errorList = errors.toList()
        return if (errorList.isEmpty()) {
            TransferResult(true, totalDownloaded.get(), "")
        } else {
            TransferResult(false, totalDownloaded.get(), "", errorList.joinToString("; "))
        }
    }

    private fun getContentLength(host: String, port: Int, path: String): Long? {
        return try {
            SocketChannel.open().use { ch ->
                ch.socket().apply {
                    setReceiveBufferSize(65536)
                    soTimeout = 10_000
                }
                ch.connect(InetSocketAddress(host, port))
                val req = "HEAD $path HTTP/1.1\r\nHost: $host\r\nConnection: close\r\n\r\n"
                ch.write(ByteBuffer.wrap(req.toByteArray(StandardCharsets.US_ASCII)))
                val buf = ByteBuffer.allocate(4096)
                ch.read(buf)
                buf.flip()
                val resp = StandardCharsets.US_ASCII.decode(buf).toString()
                val clMatch = Regex("Content-Length:\\s*(\\d+)", RegexOption.IGNORE_CASE).find(resp)
                clMatch?.groupValues?.get(1)?.toLongOrNull()
            }
        } catch (e: Exception) {
            Log.e(TAG, "HEAD failed: ${e.message}")
            null
        }
    }

    // ═══════════════════════════════════════════════════
    //  PROTOCOL HELPERS
    // ═══════════════════════════════════════════════════

    private data class Frame(val type: Int, val data: ByteArray)

    private fun sendFrame(channel: SocketChannel, type: Int, payload: ByteBuffer) {
        val len = payload.remaining()
        val header = ByteBuffer.allocate(8)
        header.putInt(type)
        header.putInt(len)
        header.flip()
        while (header.hasRemaining()) channel.write(header)
        while (payload.hasRemaining()) channel.write(payload)
    }

    private fun readFrame(channel: SocketChannel): Frame? {
        val inputStream = channel.socket().getInputStream()
        val headerBytes = ByteArray(8)
        var offset = 0
        while (offset < 8) {
            val n = inputStream.read(headerBytes, offset, 8 - offset)
            if (n < 0) return null
            offset += n
        }
        val headerBuf = ByteBuffer.wrap(headerBytes)
        val type = headerBuf.getInt()
        val length = headerBuf.getInt()
        if (length < 0 || length > MAX_FRAME_SIZE) return null

        val dataBytes = ByteArray(length)
        var dataOffset = 0
        while (dataOffset < length) {
            val n = inputStream.read(dataBytes, dataOffset, length - dataOffset)
            if (n < 0) return null
            dataOffset += n
        }
        return Frame(type, dataBytes)
    }

    // Peered capabilities cache (set by HTTP handshake)
    private var peerCapabilities: org.json.JSONObject? = null

    private fun handleFileClient(client: SocketChannel, outputDir: File) {
        try {
            client.socket().apply {
                setReceiveBufferSize(SOCKET_BUFFER_SIZE)
                setSendBufferSize(SOCKET_BUFFER_SIZE)
                soTimeout = 10_000 // Use short 10s timeout for initial peek
            }

            // Peek at first 4 bytes to detect HTTP vs raw frame protocol
            val peekBuf = ByteBuffer.allocate(4)
            drainFully(client, peekBuf)
            client.socket().soTimeout = 300_000 // Restore 5m timeout for transfer
            if (peekBuf.position() < 4) return
            peekBuf.flip()
            val peekBytes = ByteArray(4)
            peekBuf.get(peekBytes)
            val peekStr = String(peekBytes, StandardCharsets.US_ASCII)

            if (peekStr == "GET " || peekStr == "POST" || peekStr == "HEAD") {
                handleHttpClient(client, outputDir, peekStr)
                return
            }

            // ── Raw frame protocol ──
            val frameType = ByteBuffer.wrap(peekBytes).getInt()
            if (frameType != FRAME_METADATA) {
                sendFrame(client, FRAME_ERROR, ByteBuffer.wrap("Expected metadata".toByteArray()))
                return
            }
            val lenBuf = ByteBuffer.allocate(4)
            drainFully(client, lenBuf)
            lenBuf.flip()
            val frameLength = lenBuf.getInt()
            if (frameLength < 0 || frameLength > MAX_FRAME_SIZE) return
            val dataBuf = ByteBuffer.allocate(frameLength)
            drainFully(client, dataBuf)
            dataBuf.flip()
            val data = ByteArray(frameLength)
            dataBuf.get(data)
            val meta = String(data, StandardCharsets.UTF_8)
            val metaJson = org.json.JSONObject(meta)
            var filename = metaJson.optString("filename", "received_file")
            filename = filename
                    .replace("../", "").replace("..\\", "")
                    .replace("/", "_").replace("\\", "_")
                    .take(255)

            val base = resolveOutputFile(File(outputDir, filename))
            base.parentFile?.mkdirs()
            val fileChannel = FileChannel.open(base.toPath(),
                java.nio.file.StandardOpenOption.WRITE,
                java.nio.file.StandardOpenOption.CREATE,
                java.nio.file.StandardOpenOption.TRUNCATE_EXISTING)

            var totalReceived = 0L
            while (true) {
                val frame = readFrame(client) ?: break
                when (frame.type) {
                    FRAME_DATA -> {
                        val bb = ByteBuffer.wrap(frame.data)
                        while (bb.hasRemaining()) fileChannel.write(bb)
                        totalReceived += frame.data.size
                    }
                    FRAME_END -> break
                    FRAME_ERROR -> {
                        Log.e(TAG, "Client error: ${String(frame.data, StandardCharsets.UTF_8)}")
                        fileChannel.close()
                        base.delete()
                        return
                    }
                }
            }
            fileChannel.close()
            sendFrame(client, FRAME_END, ByteBuffer.wrap("OK".toByteArray()))
            Log.i(TAG, "Raw received: $base ($totalReceived bytes)")
        } catch (e: Exception) {
            Log.e(TAG, "File client handler error: ${e.message}")
        } catch (t: Throwable) {
            Log.e(TAG, "File client handler fatal: ${t.message}")
        } finally {
            try { client.close() } catch (_: Exception) {}
        }
    }

    private fun drainFully(channel: SocketChannel, buf: ByteBuffer) {
        val inputStream = channel.socket().getInputStream()
        val bytes = ByteArray(buf.remaining())
        var offset = 0
        while (offset < bytes.size) {
            val n = inputStream.read(bytes, offset, bytes.size - offset)
            if (n < 0) return
            offset += n
        }
        buf.put(bytes)
    }

    // ═══════════════════════════════════════════════════
    //  HTTP CLIENT — capabilities, telemetry, receive
    // ═══════════════════════════════════════════════════

    private fun handleHttpClient(client: SocketChannel, outputDir: File, methodPrefix: String) {
        try {
            val socket = client.socket()
            socket.setSoLinger(true, 1)
            val inputStream = socket.getInputStream()
            val outputStream = socket.getOutputStream()

            // Read raw request line + headers (no buffered reader — avoids body corruption)
            val headerBytes = mutableListOf<Byte>()
            val crlf = "\r\n".toByteArray()
            var headerComplete = false
            while (!headerComplete) {
                val b = inputStream.read()
                if (b < 0) return
                headerBytes.add(b.toByte())
                // 64KB max header to prevent memory exhaustion
                if (headerBytes.size > MAX_HEADER_SIZE) {
                    sendHttpResponse(outputStream, 400, "text/plain", "Header too large".toByteArray())
                    android.util.Log.w(TAG, "Rejected oversized HTTP header (${headerBytes.size} bytes)")
                    return
                }
                // Check for CRLF at end
                val sz = headerBytes.size
                if (sz >= 4 && headerBytes[sz - 4] == crlf[0] && headerBytes[sz - 3] == crlf[1] &&
                    headerBytes[sz - 2] == crlf[0] && headerBytes[sz - 1] == crlf[1]) {
                    headerComplete = true
                }
            }

            val headerStr = String(headerBytes.toByteArray(), StandardCharsets.US_ASCII)
            val lines = headerStr.split("\r\n")
            if (lines.isEmpty()) return

            val requestLine = methodPrefix + lines[0]
            val parts = requestLine.split(' ')
            if (parts.size < 2) return
            val method = parts[0]
            val path = parts[1]

            val headers = mutableMapOf<String, String>()
            for (i in 1 until lines.size - 2) {
                val line = lines[i]
                val colonIdx = line.indexOf(':')
                if (colonIdx > 0) {
                    headers[line.substring(0, colonIdx).trim().lowercase()] =
                        line.substring(colonIdx + 1).trim()
                }
            }

            val contentLength = headers["content-length"]?.toLongOrNull() ?: 0L

            when {
                method == "GET" && path == "/api/capabilities" -> {
                    val caps = buildCapabilityExchange()
                    sendHttpResponse(outputStream, 200, "application/json", caps.toString().toByteArray())
                }
                method == "GET" && path == "/api/telemetry" -> {
                    val tel = buildTelemetry()
                    sendHttpResponse(outputStream, 200, "application/json", tel.toString().toByteArray())
                }
                method == "GET" && path.startsWith("/api/list") -> {
                    val query = path.substringAfter("?")
                    val dirPath = query.split("&").find { it.startsWith("path=") }?.substring(5) ?: "/"
                    val decodedPath = try { java.net.URLDecoder.decode(dirPath, "UTF-8") } catch (_: Exception) { dirPath }
                    val file = File(decodedPath)
                    if (file.exists() && file.isDirectory) {
                        val arr = org.json.JSONArray()
                        file.listFiles()?.forEach { child ->
                            arr.put(org.json.JSONObject().apply {
                                put("name", child.name)
                                put("path", child.absolutePath)
                                put("is_dir", child.isDirectory)
                                put("is_file", child.isFile)
                                put("size", child.length())
                            })
                        }
                        sendHttpResponse(outputStream, 200, "application/json", arr.toString().toByteArray())
                    } else {
                        sendHttpResponse(outputStream, 404, "application/json", "{\"error\":\"Directory not found\"}".toByteArray())
                    }
                }
                method == "GET" && path.startsWith("/api/files/") -> {
                    val filename = path.substring("/api/files/".length)
                    val file = File(outputDir, filename)
                    if (file.exists() && file.isFile) {
                        val fileSize = file.length()
                        val response = "HTTP/1.1 200 OK\r\nContent-Length: $fileSize\r\nContent-Disposition: attachment; filename=\"$filename\"\r\nConnection: close\r\n\r\n"
                        outputStream.write(response.toByteArray(StandardCharsets.US_ASCII))
                        FileChannel.open(file.toPath(), StandardOpenOption.READ).use { fc ->
                            val buf = ByteArray(65536)
                            var n = fc.read(ByteBuffer.wrap(buf))
                            while (n > 0) {
                                outputStream.write(buf, 0, n)
                                n = fc.read(ByteBuffer.wrap(buf))
                            }
                        }
                        outputStream.flush()
                        socket.close()
                    } else {
                        sendHttpResponse(outputStream, 404, "text/plain", "File not found".toByteArray())
                    }
                }
                method == "POST" && path == "/api/handshake" -> {
                    val body = if (contentLength > 0 && contentLength <= MAX_BODY_SIZE) {
                        val buf = ByteArray(contentLength.toInt())
                        readFully(inputStream, buf)
                        String(buf, StandardCharsets.UTF_8)
                    } else if (contentLength > MAX_BODY_SIZE) {
                        android.util.Log.w(TAG, "Rejected oversized handshake body: ${contentLength} bytes")
                        sendHttpResponse(outputStream, 413, "text/plain", "Body too large".toByteArray())
                        return
                    } else ""
                    if (body.isNotEmpty()) {
                        try { peerCapabilities = org.json.JSONObject(body) } catch (_: Exception) {}
                    }
                    val ourCaps = buildCapabilityExchange()
                    sendHttpResponse(outputStream, 200, "application/json", ourCaps.toString().toByteArray())
                }
                method == "HEAD" && path == "/" -> {
                    sendHttpResponse(outputStream, 200, "text/plain", ByteArray(0))
                }
                method == "POST" && path == "/api/receive-file" -> {
                    val rawFilename = headers["x-filename"] ?: "received_file"
                    var filename = java.net.URLDecoder.decode(rawFilename, "UTF-8")
                        .replace("/", "_").replace("\\", "_").take(255)
                    // Prevent path traversal via canonical path check
                    var outFile = resolveOutputFile(File(outputDir, filename))
                    try {
                        val canonical = outFile.canonicalPath
                        val downloadCanonical = outputDir.canonicalPath
                        if (!canonical.startsWith(downloadCanonical)) {
                            android.util.Log.w(TAG, "Rejected path traversal: $filename -> $canonical")
                            filename = filename.replace("..", "_")
                            outFile = resolveOutputFile(File(outputDir, filename))
                        }
                    } catch (_: Exception) {}
                    outFile.parentFile?.mkdirs()

                    var totalBytes = 0L
                    java.io.BufferedOutputStream(FileOutputStream(outFile), 1048576).use { bos ->
                        val bis = java.io.BufferedInputStream(inputStream, 1048576)
                        val buf = ByteArray(1048576) // 1MB array
                        var remaining = contentLength
                        while (remaining > 0) {
                            val toRead = minOf(buf.size.toLong(), remaining).toInt()
                            val n = bis.read(buf, 0, toRead)
                            if (n <= 0) break
                            bos.write(buf, 0, n)
                            remaining -= n
                            totalBytes += n
                        }
                    }
                    val resp = """{"saved_to":"${outFile.absolutePath}","size":$totalBytes,"success":true}"""
                    sendHttpResponse(outputStream, 200, "application/json", resp.toByteArray())
                    Log.i(TAG, "HTTP received: $outFile ($totalBytes bytes)")
                }
                else -> {
                    sendHttpResponse(outputStream, 404, "text/plain", "Not Found".toByteArray())
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "HTTP handler error: ${e.message}")
        } catch (t: Throwable) {
            Log.e(TAG, "HTTP handler fatal: ${t.message}")
        }
    }

    private fun readFully(inputStream: InputStream, buf: ByteArray) {
        var offset = 0
        while (offset < buf.size) {
            val n = inputStream.read(buf, offset, buf.size - offset)
            if (n <= 0) return
            offset += n
        }
    }

    private fun sendHttpResponse(outputStream: java.io.OutputStream, statusCode: Int, contentType: String, body: ByteArray) {
        val reason = when (statusCode) { 200 -> "OK"; 400 -> "Bad Request"; 404 -> "Not Found"; else -> "Unknown" }
        val response = "HTTP/1.1 $statusCode $reason\r\nContent-Type: $contentType\r\nContent-Length: ${body.size}\r\nConnection: close\r\n\r\n"
        outputStream.write(response.toByteArray())
        if (body.isNotEmpty()) outputStream.write(body)
        outputStream.flush()
    }

    private fun buildCapabilityExchange(): org.json.JSONObject {
        val state = buildTelemetry()
        return org.json.JSONObject().apply {
            put("protocol_version", "1.0")
            put("node_id", "android-${Build.MODEL}")
            put("hardware", org.json.JSONObject().apply {
                put("cpu_architecture", System.getProperty("os.arch") ?: "arm64")
                put("cpu_cores", Runtime.getRuntime().availableProcessors())
                put("cpu_performance_cores", 0)
                put("cpu_efficiency_cores", 0)
                put("ram_mb", Runtime.getRuntime().maxMemory() / 1_048_576)
                put("storage_type", "ufs")
                put("storage_read_mbps", 800.0)
                put("storage_write_mbps", 600.0)
                put("storage_free_gb", 50.0)
            })
            put("network", org.json.JSONObject().apply {
                put("interface_type", "WiFi6")
                put("link_speed_mbps", 800.0)
                put("rtt_ms", 0.0)
                put("measured_bandwidth_mbps", 0.0)
                put("mtu", 1500)
            })
            put("state", state)
            put("features", org.json.JSONObject().apply {
                put("zero_copy", true)
                put("parallel_upload", true)
                put("parallel_download", true)
                put("resume", true)
                put("streaming_directory", true)
                put("compression", org.json.JSONArray(listOf("none")))
                put("integrity", org.json.JSONArray(listOf("sha256")))
                put("http2", false)
                put("http3", false)
            })
        }
    }

    private fun buildTelemetry(): org.json.JSONObject {
        return org.json.JSONObject().apply {
            put("battery_pct", 100.0)
            put("charging", true)
            put("thermal_state", "nominal")
            put("cpu_load_pct", 0.0)
            put("memory_pressure", "low")
            put("disk_utilization_pct", 0.0)
        }
    }

    private fun resolveOutputFile(file: File): File {
        if (!file.exists()) return file
        var counter = 1
        val dot = file.name.lastIndexOf('.')
        while (true) {
            val candidate = if (dot > 0) {
                File(file.parentFile, file.name.substring(0, dot) + "_$counter" + file.name.substring(dot))
            } else {
                File(file.parentFile, file.name + "_$counter")
            }
            if (!candidate.exists()) return candidate
            counter++
        }
    }

    // ═══════════════════════════════════════════════════
    //  IPv6 — resolve host to all available addresses
    // ═══════════════════════════════════════════════════

    fun resolveHost(host: String): String {
        return try {
            val addrs = InetAddress.getAllByName(host)
            // Prefer IPv4, fall back to IPv6
            val ipv4 = addrs.find { it !is Inet6Address }
            val ipv6 = addrs.find { it is Inet6Address }
            (ipv4 ?: ipv6 ?: addrs.firstOrNull())?.hostAddress ?: host
        } catch (_: Exception) { host }
    }

    fun resolveSocketAddress(host: String, port: Int): InetSocketAddress {
        return try {
            val addrs = InetAddress.getAllByName(host)
            val ipv4 = addrs.find { it !is Inet6Address }
            val addr = ipv4 ?: addrs.firstOrNull() ?: InetAddress.getByName(host)
            InetSocketAddress(addr, port)
        } catch (_: Exception) {
            InetSocketAddress(host, port)
        }
    }

    // ═══════════════════════════════════════════════════
    //  Speed throttle — sleep to hit maxBytesPerSecond
    // ═══════════════════════════════════════════════════

    fun applyThrottle(bytesThisInterval: Long, intervalStart: Long, maxBytesPerSecond: Long) {
        if (maxBytesPerSecond <= 0) return
        val elapsed = System.currentTimeMillis() - intervalStart
        val expectedTime = (bytesThisInterval.toDouble() / maxBytesPerSecond * 1000).toLong()
        if (elapsed < expectedTime) {
            try { Thread.sleep(expectedTime - elapsed) } catch (_: Exception) {}
        }
    }

    // ═══════════════════════════════════════════════════
    //  Resume download with Range header
    // ═══════════════════════════════════════════════════

    fun httpDownloadWithResume(
        url: String,
        outputFile: File,
        existingBytes: Long = 0L,
        expectedChecksum: String = "",
        maxBytesPerSecond: Long = 0L,
        progressCallback: ((Long) -> Unit)? = null
    ): TransferResult {
        if (url.startsWith("https://")) {
            return httpsDownload(url, outputFile, expectedChecksum, progressCallback)
        }

        val uri = java.net.URI(url)
        val host = uri.host ?: return TransferResult(false, 0, "", "Invalid URL")
        val port = uri.port.takeIf { it > 0 } ?: 80
        val path = uri.rawPath + (uri.rawQuery?.let { "?$it" } ?: "")

        val fileSize = AtomicLong(existingBytes)
        val sha256 = if (expectedChecksum.isNotEmpty()) MessageDigest.getInstance("SHA-256") else null
        outputFile.parentFile?.mkdirs()

        val openOpts = mutableListOf(
            java.nio.file.StandardOpenOption.WRITE,
            java.nio.file.StandardOpenOption.CREATE
        )
        if (existingBytes == 0L) openOpts.add(java.nio.file.StandardOpenOption.TRUNCATE_EXISTING)
        val fileChannel = FileChannel.open(outputFile.toPath(), *openOpts.toTypedArray())
        if (existingBytes > 0) fileChannel.position(existingBytes)

        SocketChannel.open().use { channel ->
            channel.socket().apply {
                setReceiveBufferSize(SOCKET_BUFFER_SIZE)
                setSendBufferSize(SOCKET_BUFFER_SIZE)
                setTcpNoDelay(true)
                soTimeout = 300_000
            }
            channel.connect(resolveSocketAddress(host, port))

            var request = "GET $path HTTP/1.1\r\nHost: $host\r\nConnection: close\r\n"
            if (existingBytes > 0) request += "Range: bytes=$existingBytes-\r\n"
            request += "\r\n"
            val reqBuf = ByteBuffer.wrap(request.toByteArray(StandardCharsets.US_ASCII))
            while (reqBuf.hasRemaining()) channel.write(reqBuf)

            val headerBuf = ByteBuffer.allocateDirect(8192)
            var headerData = byteArrayOf()
            var headersComplete = false
            var throttleStart = System.currentTimeMillis()
            var throttleBytes = 0L

            while (!headersComplete) {
                headerBuf.clear()
                val bytesRead = channel.read(headerBuf)
                if (bytesRead <= 0) throw java.io.IOException("Server closed while reading headers")
                headerBuf.flip()
                val chunk = ByteArray(headerBuf.remaining())
                headerBuf.get(chunk)
                headerData += chunk

                val headerStr = String(headerData, StandardCharsets.US_ASCII)
                val endIdx = headerStr.indexOf("\r\n\r\n")
                if (endIdx >= 0) {
                    headersComplete = true
                    val statusCode = Regex("HTTP/1\\.[01] (\\d{3})")
                        .find(headerStr)?.groupValues?.get(1)?.toIntOrNull() ?: 0
                    if (statusCode !in 200..299) {
                        fileChannel.close()
                        return TransferResult(false, 0, "", "Server returned $statusCode")
                    }
                    // If server sent full content (200) instead of partial (206), restart from 0
                    if (existingBytes > 0 && statusCode == 200) {
                        fileChannel.position(0)
                        fileSize.set(0)
                    }
                    val bodyStart = endIdx + 4
                    val leftover = headerData.size - bodyStart
                    if (leftover > 0) {
                        val leftoverBytes = headerData.copyOfRange(bodyStart, headerData.size)
                        if (leftoverBytes.isNotEmpty()) {
                            val bb = ByteBuffer.wrap(leftoverBytes)
                            while (bb.hasRemaining()) fileChannel.write(bb)
                            fileSize.addAndGet(leftover.toLong())
                            sha256?.update(leftoverBytes)
                            throttleBytes += leftover
                            applyThrottle(throttleBytes, throttleStart, maxBytesPerSecond)
                            progressCallback?.invoke(fileSize.get())
                        }
                    }
                }
            }

            var totalRead = fileSize.get()
            while (true) {
                throttleStart = System.currentTimeMillis()
                throttleBytes = 0L
                val count = fileChannel.transferFrom(channel, totalRead, TRANSFER_CHUNK_SIZE.toLong())
                if (count <= 0) break
                totalRead += count
                fileSize.addAndGet(count)
                throttleBytes += count
                applyThrottle(throttleBytes, throttleStart, maxBytesPerSecond)
                progressCallback?.invoke(totalRead)
            }
            fileChannel.close()

            val digest = sha256?.digest()?.joinToString("") { "%02x".format(it) } ?: ""
            val checksumOk = expectedChecksum.isEmpty() || digest == expectedChecksum
            if (expectedChecksum.isNotEmpty() && !checksumOk) {
                outputFile.delete()
                return TransferResult(false, totalRead, digest, "Checksum mismatch")
            }
            return TransferResult(true, totalRead, digest)
        }
    }

    // ═══════════════════════════════════════════════════
    //  ZIP — create zip from content URIs
    // ═══════════════════════════════════════════════════

    fun createZipFromUris(
        uris: List<android.net.Uri>,
        filenames: List<String>,
        outputZip: File,
        contentResolver: android.content.ContentResolver
    ): File {
        outputZip.parentFile?.mkdirs()
        ZipOutputStream(FileOutputStream(outputZip)).use { zos ->
            for (i in uris.indices) {
                val uri = uris[i]
                val entryName = filenames.getOrElse(i) { "file_$i" }
                zos.putNextEntry(ZipEntry(entryName))
                try {
                    contentResolver.openInputStream(uri)?.use { input ->
                        input.copyTo(zos)
                    }
                } catch (_: Exception) {}
                zos.closeEntry()
            }
        }
        return outputZip
    }

    fun createZipFromFiles(files: List<File>): File {
        val outputZip = File(files.first().parentFile ?: File("."), "transfer_bundle.zip")
        outputZip.parentFile?.mkdirs()
        ZipOutputStream(FileOutputStream(outputZip)).use { zos ->
            for (file in files) {
                zos.putNextEntry(ZipEntry(file.name))
                FileInputStream(file).use { it.copyTo(zos) }
                zos.closeEntry()
            }
        }
        return outputZip
    }

    fun unzipToDir(zipFile: File, outputDir: File): List<String> {
        outputDir.mkdirs()
        val extracted = mutableListOf<String>()
        try {
            java.util.zip.ZipInputStream(FileInputStream(zipFile)).use { zis ->
                var entry = zis.nextEntry
                while (entry != null) {
                    val sanitizedName = entry.name
                        .replace("../", "").replace("..\\", "")
                        .replace("/", "_").replace("\\", "_")
                    val outFile = File(outputDir, sanitizedName)
                    // Guard: ensure output stays within outputDir
                    if (!outFile.canonicalPath.startsWith(outputDir.canonicalPath + File.separator)) {
                        entry = zis.nextEntry
                        continue
                    }
                    outFile.parentFile?.mkdirs()
                    FileOutputStream(outFile).use { zis.copyTo(it) }
                    extracted.add(outFile.absolutePath)
                    entry = zis.nextEntry
                }
            }
        } catch (_: Exception) {}
        return extracted
    }

    fun restartFileServer(outputDir: File): java.nio.channels.ServerSocketChannel {
        return startFileServer(outputDir)
    }

    // ═══════════════════════════════════════════════════
    //  CAPABILITY HANDSHAKE
    // ═══════════════════════════════════════════════════

    data class HardwareCaps(
        val cpu_architecture: String = "arm64",
        val cpu_cores: Int = Runtime.getRuntime().availableProcessors(),
        val cpu_performance_cores: Int = 0,
        val cpu_efficiency_cores: Int = 0,
        val ram_mb: Long = Runtime.getRuntime().maxMemory() / 1_048_576,
        val storage_type: String = "ufs",
        val storage_read_mbps: Double = 800.0,
        val storage_write_mbps: Double = 600.0,
        val storage_free_gb: Double = 50.0
    )

    data class NetworkCaps(
        val interface_type: String = "WiFi6",
        val link_speed_mbps: Double = 800.0,
        val rtt_ms: Double = 0.0,
        val measured_bandwidth_mbps: Double = 0.0,
        val mtu: Int = 1500
    )

    data class TelemetryState(
        val battery_pct: Double = 100.0,
        val charging: Boolean = true,
        val thermal_state: String = "nominal",
        val cpu_load_pct: Double = 0.0,
        val memory_pressure: String = "low",
        val disk_utilization_pct: Double = 0.0
    )

    data class Features(
        val zero_copy: Boolean = true,
        val parallel_upload: Boolean = true,
        val parallel_download: Boolean = true,
        val resume: Boolean = true,
        val streaming_directory: Boolean = true,
        val compression: List<String> = listOf("none"),
        val integrity: List<String> = listOf("sha256"),
        val http2: Boolean = false,
        val http3: Boolean = false
    )

    data class CapabilityExchange(
        val protocol_version: String = "1.0",
        val node_id: String = "android-${android.os.Build.MODEL}",
        val hardware: HardwareCaps = HardwareCaps(),
        val network: NetworkCaps = NetworkCaps(),
        val state: TelemetryState = TelemetryState(),
        val features: Features = Features()
    )

    fun performHandshake(host: String, port: Int): CapabilityExchange? {
        val caps = CapabilityExchange()
        val json = org.json.JSONObject().apply {
            put("protocol_version", caps.protocol_version)
            put("node_id", caps.node_id)
            put("hardware", org.json.JSONObject().apply {
                put("cpu_architecture", caps.hardware.cpu_architecture)
                put("cpu_cores", caps.hardware.cpu_cores)
                put("cpu_performance_cores", caps.hardware.cpu_performance_cores)
                put("cpu_efficiency_cores", caps.hardware.cpu_efficiency_cores)
                put("ram_mb", caps.hardware.ram_mb)
                put("storage_type", caps.hardware.storage_type)
                put("storage_read_mbps", caps.hardware.storage_read_mbps)
                put("storage_write_mbps", caps.hardware.storage_write_mbps)
                put("storage_free_gb", caps.hardware.storage_free_gb)
            })
            put("network", org.json.JSONObject().apply {
                put("interface_type", caps.network.interface_type)
                put("link_speed_mbps", caps.network.link_speed_mbps)
                put("rtt_ms", caps.network.rtt_ms)
                put("measured_bandwidth_mbps", caps.network.measured_bandwidth_mbps)
                put("mtu", caps.network.mtu)
            })
            put("state", org.json.JSONObject().apply {
                put("battery_pct", caps.state.battery_pct)
                put("charging", caps.state.charging)
                put("thermal_state", caps.state.thermal_state)
                put("cpu_load_pct", caps.state.cpu_load_pct)
                put("memory_pressure", caps.state.memory_pressure)
                put("disk_utilization_pct", caps.state.disk_utilization_pct)
            })
            put("features", org.json.JSONObject().apply {
                put("zero_copy", caps.features.zero_copy)
                put("parallel_upload", caps.features.parallel_upload)
                put("parallel_download", caps.features.parallel_download)
                put("resume", caps.features.resume)
                put("streaming_directory", caps.features.streaming_directory)
                put("compression", org.json.JSONArray(caps.features.compression))
                put("integrity", org.json.JSONArray(caps.features.integrity))
                put("http2", caps.features.http2)
                put("http3", caps.features.http3)
            })
        }.toString()

        return try {
            val socket = Socket()
            socket.connect(InetSocketAddress(host, port), 5000)
            socket.soTimeout = 5000

            val request = """POST /api/handshake HTTP/1.1
Host: $host:$port
Content-Type: application/json
Content-Length: ${json.toByteArray().size}
Connection: close

$json""".replace("\n", "\r\n")

            socket.getOutputStream().write(request.toByteArray())
            socket.getOutputStream().flush()

            val response = socket.getInputStream().bufferedReader().use { it.readText() }
            val bodyStart = response.indexOf("\r\n\r\n") + 4
            if (response.contains("200 OK") && bodyStart > 4) {
                val body = response.substring(bodyStart)
                val peerJson = org.json.JSONObject(body)
                // Parse and return peer capabilities
                CapabilityExchange(
                    node_id = peerJson.optString("node_id", "unknown"),
                    hardware = HardwareCaps(
                        cpu_cores = peerJson.optJSONObject("hardware")?.optInt("cpu_cores", 0) ?: 0,
                        ram_mb = peerJson.optJSONObject("hardware")?.optLong("ram_mb", 0) ?: 0,
                        storage_type = peerJson.optJSONObject("hardware")?.optString("storage_type", "unknown") ?: "unknown",
                        storage_write_mbps = peerJson.optJSONObject("hardware")?.optDouble("storage_write_mbps", 0.0) ?: 0.0,
                    ),
                    network = NetworkCaps(
                        interface_type = peerJson.optJSONObject("network")?.optString("interface_type", "unknown") ?: "unknown",
                        link_speed_mbps = peerJson.optJSONObject("network")?.optDouble("link_speed_mbps", 0.0) ?: 0.0,
                    ),
                    features = Features(
                        resume = peerJson.optJSONObject("features")?.optBoolean("resume", false) ?: false,
                        streaming_directory = peerJson.optJSONObject("features")?.optBoolean("streaming_directory", false) ?: false,
                    )
                )
            } else null
        } catch (e: Exception) {
            Log.e(TAG, "Handshake failed: ${e.message}")
            null
        }
    }

    // ═══════════════════════════════════════════════════
    //  STREAMING DIRECTORY RECEIVE
    // ═══════════════════════════════════════════════════

    data class DirectoryFileEntry(
        val relativePath: String,
        val size: Long,
        val data: ByteArray?
    )

    fun receiveDirectoryStream(
        host: String,
        port: Int,
        outputDir: File,
        progressCallback: ((Long, Long) -> Unit)? = null
    ): TransferResult {
        return try {
            val socket = Socket()
            socket.connect(InetSocketAddress(host, port), 10000)
            socket.soTimeout = 30000

            // Send request
            val request = "GET /api/receive-directory-stream HTTP/1.1\r\nHost: $host:$port\r\nConnection: close\r\n\r\n"
            socket.getOutputStream().write(request.toByteArray())
            socket.getOutputStream().flush()

            val inputStream = socket.getInputStream()
            val reader = inputStream.bufferedReader()

            // Read HTTP response headers
            val headerLines = mutableListOf<String>()
            var line = reader.readLine()
            while (line != null && line.isNotEmpty()) {
                headerLines.add(line)
                line = reader.readLine()
            }

            if (!headerLines.firstOrNull().orEmpty().contains("200")) {
                return TransferResult(false, 0, "", "Server returned non-200")
            }

            var totalBytes: Long = 0
            var fileCount = 0
            outputDir.mkdirs()

            // Parse streaming directory entries
            while (true) {
                // Read file: header
                val fileLine = reader.readLine() ?: break
                if (fileLine == "." || fileLine == "\r\n.") break // end marker
                if (!fileLine.startsWith("file: ")) continue

                val relPath = fileLine.removePrefix("file: ")
                val sizeLine = reader.readLine() ?: break
                val fileSize = sizeLine.removePrefix("size: ").toLongOrNull() ?: 0
                val blankLine = reader.readLine() // consume blank line

                // Read file data
                val outFile = File(outputDir, relPath)
                outFile.parentFile?.mkdirs()
                FileOutputStream(outFile).use { fos ->
                    val buf = ByteArray(8192)
                    var remaining = fileSize
                    while (remaining > 0) {
                        val toRead = minOf(buf.size.toLong(), remaining).toInt()
                        val n = inputStream.read(buf, 0, toRead)
                        if (n == -1) break
                        fos.write(buf, 0, n)
                        remaining -= n
                        totalBytes += n
                        progressCallback?.invoke(totalBytes, fileSize)
                    }
                }
                fileCount++
            }

            Log.i(TAG, "Directory stream received: $fileCount files, ${totalBytes} bytes")
            TransferResult(true, totalBytes, "streamed")
        } catch (e: Exception) {
            Log.e(TAG, "Directory stream failed: ${e.message}")
            TransferResult(false, 0, "", e.message ?: "Unknown error")
        }
    }
}
