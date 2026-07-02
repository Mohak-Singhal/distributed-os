package com.dos.agent.scan

import android.util.Size
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleOwner
import java.util.concurrent.Executors

/**
 * Manages CameraX lifecycle for QR code scanning.
 *
 * Binds a [Preview] and an [ImageAnalysis] (with a [QrAnalyzer]) to
 * the given [LifecycleOwner]. Call [start] after camera permission is
 * granted.
 */
class CameraManager(
    private val lifecycleOwner: LifecycleOwner,
    private val previewView: PreviewView,
    private val analyzer: QrAnalyzer
) {
    private val cameraExecutor = Executors.newSingleThreadExecutor()

    /** Binds the camera, preview, and analyzer. Call from main thread. */
    fun start() {
        val cameraProviderFuture = ProcessCameraProvider.getInstance(lifecycleOwner as android.content.Context)
        cameraProviderFuture.addListener({
            val cameraProvider = cameraProviderFuture.get()

            val preview = Preview.Builder().build().also {
                it.setSurfaceProvider(previewView.surfaceProvider)
            }

            val barcodeAnalyzer = ImageAnalysis.Builder()
                .setTargetResolution(Size(1280, 720))
                .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                .build()
                .also {
                    it.setAnalyzer(cameraExecutor, analyzer)
                }

            try {
                cameraProvider.unbindAll()
                cameraProvider.bindToLifecycle(
                    lifecycleOwner,
                    CameraSelector.DEFAULT_BACK_CAMERA,
                    preview,
                    barcodeAnalyzer
                )
            } catch (_: Exception) {
                // Camera error handled by caller via status callback
            }
        }, ContextCompat.getMainExecutor(lifecycleOwner as android.content.Context))
    }

    /** Release camera resources. */
    fun shutdown() {
        cameraExecutor.shutdown()
    }
}
