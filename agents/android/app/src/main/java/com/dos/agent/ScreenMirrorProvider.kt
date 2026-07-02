package com.dos.agent

import android.content.Context
import android.hardware.display.DisplayManager
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.Surface
import java.io.OutputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.nio.ByteBuffer
import java.util.concurrent.atomic.AtomicBoolean

class ScreenMirrorProvider(private val context: Context) {

    companion object {
        private const val TAG = "ScreenMirror"
        private const val VIDEO_PORT = 7892
        private const val BITRATE = 5_000_000
        private const val FRAME_RATE = 30
        private const val I_FRAME_INTERVAL = 2

        private const val OUTPUT_FORMAT_CHANGED = -2
        private const val INFO_TRY_AGAIN_LATER = -1

        private var mediaProjection: MediaProjection? = null
        private var serverSocket: ServerSocket? = null
        private var isRunning = AtomicBoolean(false)
        private var clientSocket: Socket? = null
    }

    private val mainHandler = Handler(Looper.getMainLooper())

    var onStopped: (() -> Unit)? = null

    fun setMediaProjection(mp: MediaProjection) {
        mediaProjection = mp
    }

    fun isProjectionSet(): Boolean = mediaProjection != null

    fun start(timeoutSeconds: Long = 0) {
        if (isRunning.get()) {
            Log.w(TAG, "Already running")
            return
        }
        isRunning.set(true)
        Thread({
            try {
                serverSocket = ServerSocket(VIDEO_PORT, 50, InetAddress.getByName("0.0.0.0"))
                Log.i(TAG, "Screen mirror server listening on port $VIDEO_PORT")

                while (true) {
                    val socket = serverSocket?.accept() ?: break
                    clientSocket = socket
                    Log.i(TAG, "Client connected: ${socket.inetAddress}")
                    startStreaming(socket)
                    break
                }
            } catch (e: Exception) {
                Log.e(TAG, "Server error: ${e.message}")
            } finally {
                isRunning.set(false)
            }
        }, "ScreenMirror-Server").start()
    }

    fun stop() {
        isRunning.set(false)
        try {
            clientSocket?.close()
        } catch (_: Exception) {}
        try {
            serverSocket?.close()
        } catch (_: Exception) {}
        mediaProjection?.stop()
    }

    private fun startStreaming(socket: Socket) {
        val projection = mediaProjection ?: run {
            Log.e(TAG, "MediaProjection not set")
            return
        }

        val outputStream = socket.getOutputStream()
        val displayManager = context.getSystemService(Context.DISPLAY_SERVICE) as DisplayManager
        val displays = displayManager.getDisplays()
        val defaultDisplay = if (displays.isNotEmpty()) displays[0] else null
        val width = 1280
        val height = 720
        val dpi = 320

        val mimeType = "video/avc"

        val format = MediaFormat.createVideoFormat(mimeType, width, height).apply {
            setInteger(MediaFormat.KEY_BIT_RATE, BITRATE)
            setInteger(MediaFormat.KEY_FRAME_RATE, FRAME_RATE)
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, I_FRAME_INTERVAL)
            setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
        }

        projection.registerCallback(object : MediaProjection.Callback() {
            override fun onStop() {
                Log.w(TAG, "MediaProjection stopped by system")
                isRunning.set(false)
                mediaProjection = null
                onStopped?.invoke()
            }
        }, mainHandler)

        val encoder = MediaCodec.createEncoderByType(mimeType)
        encoder.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
        val inputSurface: Surface = encoder.createInputSurface()
        encoder.start()

        val virtualDisplay = projection.createVirtualDisplay(
            "ScreenMirror",
            width, height, dpi,
            DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR,
            inputSurface, null, null
        )

        Log.i(TAG, "Screen mirror started: ${width}x${height} @ ${FRAME_RATE}fps")

        val bufferInfo = MediaCodec.BufferInfo()
        var isRunningLocal = true

        while (isRunning.get() && isRunningLocal) {
            try {
                val outputIndex = encoder.dequeueOutputBuffer(bufferInfo, 10000)
                when {
                    outputIndex == INFO_TRY_AGAIN_LATER -> {}
                    outputIndex == OUTPUT_FORMAT_CHANGED -> {
                        val newFormat = encoder.outputFormat
                        Log.i(TAG, "Output format changed: $newFormat")
                    }
                    outputIndex >= 0 -> {
                        val outputBuffer: ByteBuffer = encoder.getOutputBuffer(outputIndex) ?: continue
                        val frameData = ByteArray(bufferInfo.size)
                        outputBuffer.position(bufferInfo.offset)
                        outputBuffer.get(frameData, 0, bufferInfo.size)

                        val sizePrefix = ByteBuffer.allocate(4).putInt(frameData.size).array()
                        try {
                            outputStream.write(sizePrefix)
                            outputStream.write(frameData)
                            outputStream.flush()
                        } catch (e: Exception) {
                            Log.w(TAG, "Client disconnected: ${e.message}")
                            isRunningLocal = false
                        }

                        encoder.releaseOutputBuffer(outputIndex, false)
                    }
                }
            } catch (e: Exception) {
                Log.e(TAG, "Encoding error: ${e.javaClass.simpleName}: ${e.message}")
                break
            }
        }

        try { virtualDisplay?.release() } catch (_: Exception) {}
        encoder.stop()
        encoder.release()
        try {
            outputStream.close()
        } catch (_: Exception) {}
        Log.i(TAG, "Screen mirror stopped")
    }
}
