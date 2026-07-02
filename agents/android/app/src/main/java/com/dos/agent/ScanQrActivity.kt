package com.dos.agent

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Bundle
import android.view.View
import android.widget.TextView
import android.widget.Toast
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.camera.view.PreviewView
import androidx.core.content.ContextCompat
import com.dos.agent.pairing.PairResult
import com.dos.agent.pairing.PairingClient
import com.dos.agent.scan.CameraManager
import com.dos.agent.scan.QrAnalyzer

/**
 * Camera-based QR code scanner for PDOS wireless pairing.
 *
 * Delegates to:
 * - [CameraManager] for CameraX lifecycle
 * - [QrAnalyzer] for ML Kit barcode detection
 * - [PairingClient] for the HTTP handshake
 */
class ScanQrActivity : AppCompatActivity() {

    private lateinit var previewView: PreviewView
    private lateinit var tvStatus: TextView
    private lateinit var scanOverlay: View
    private lateinit var cameraManager: CameraManager
    private lateinit var analyzer: QrAnalyzer
    private val pairingClient = PairingClient()

    private val requestPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { granted ->
        if (granted) startCamera()
        else {
            Toast.makeText(this, "Camera permission required", Toast.LENGTH_LONG).show()
            finish()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_scan_qr)

        previewView = findViewById(R.id.previewView)
        tvStatus = findViewById(R.id.tvScanStatus)
        scanOverlay = findViewById(R.id.scanOverlay)

        findViewById<View>(R.id.btnCloseScanner).setOnClickListener { finish() }

        analyzer = QrAnalyzer(::onQrDetected)
        cameraManager = CameraManager(this, previewView, analyzer)

        if (ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA)
            == PackageManager.PERMISSION_GRANTED
        ) {
            startCamera()
        } else {
            requestPermissionLauncher.launch(Manifest.permission.CAMERA)
        }
    }

    private fun startCamera() {
        cameraManager.start()
        tvStatus.text = "Point camera at the QR code"
        scanOverlay.visibility = View.VISIBLE
    }

    private fun onQrDetected(payload: String) {
        tvStatus.text = "Pairing..."
        handleQrPayload(payload)
    }

    private fun handleQrPayload(payload: String) {
        val uri = try {
            java.net.URI(payload)
        } catch (e: Exception) {
            tvStatus.text = "Invalid QR code"
            analyzer.reset()
            return
        }

        val host = uri.host ?: return
        val port = uri.port
        val query = uri.query ?: return
        val token = query.split("&")
            .firstOrNull { it.startsWith("token=") }
            ?.removePrefix("token=") ?: return

        if (port <= 0) {
            tvStatus.text = "Invalid port in QR code"
            analyzer.reset()
            return
        }

        pairingClient.pair(host, port, token) { result ->
            runOnUiThread {
                when (result) {
                    is PairResult.Success -> {
                        SettingsManager.addTrustedPeer(host)

                        val intent = Intent(this, MainActivity::class.java).apply {
                            flags = Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
                            putExtra("paired_host", host)
                            putExtra("paired_relay_url", result.relayUrl)
                            putExtra("paired_node_id", result.nodeId)
                            putExtra("paired_node_name", result.nodeName)
                        }
                        startActivity(intent)
                        finish()
                    }
                    is PairResult.Error -> {
                        tvStatus.text = "Pairing failed: ${result.message}"
                        analyzer.reset()
                        scanOverlay.postDelayed({
                            scanOverlay.visibility = View.VISIBLE
                        }, 2000)
                    }
                }
            }
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        cameraManager.shutdown()
    }
}
