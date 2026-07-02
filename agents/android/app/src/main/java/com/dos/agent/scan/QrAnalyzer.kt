package com.dos.agent.scan

import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import com.google.mlkit.vision.barcode.BarcodeScanning
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.common.InputImage

/**
 * CameraX [ImageAnalysis.Analyzer] that detects PDOS QR codes
 * using ML Kit Barcode Scanning.
 *
 * When a valid `pdos://` QR code is found, [onQrDetected] is called
 * with the raw payload string. Subsequent frames are ignored until
 * [reset] is called.
 */
class QrAnalyzer(
    private val onQrDetected: (String) -> Unit
) : ImageAnalysis.Analyzer {

    @Volatile
    private var isProcessing = false

    override fun analyze(imageProxy: ImageProxy) {
        if (isProcessing) {
            imageProxy.close()
            return
        }

        @android.annotation.SuppressLint("UnsafeOptInUsageError")
        val mediaImage = imageProxy.image
        if (mediaImage == null) {
            imageProxy.close()
            return
        }

        val inputImage = InputImage.fromMediaImage(mediaImage, imageProxy.imageInfo.rotationDegrees)
        val scanner = BarcodeScanning.getClient()
        val task = scanner.process(inputImage)

        task.addOnSuccessListener { barcodes ->
            for (barcode in barcodes) {
                if (barcode.valueType == Barcode.TYPE_TEXT || barcode.valueType == Barcode.TYPE_URL) {
                    val raw = barcode.rawValue ?: continue
                    if (raw.startsWith("pdos://")) {
                        isProcessing = true
                        onQrDetected(raw)
                        break
                    }
                }
            }
        }.addOnCompleteListener {
            imageProxy.close()
        }
    }

    /** Allow processing new frames (e.g. after a failed pair attempt). */
    fun reset() {
        isProcessing = false
    }
}
