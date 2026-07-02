package com.dos.agent

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Bundle
import android.widget.TextView
import com.google.android.material.progressindicator.CircularProgressIndicator
import androidx.appcompat.app.AppCompatActivity
import androidx.localbroadcastmanager.content.LocalBroadcastManager
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.google.android.material.button.MaterialButton

class ProgressPageActivity : AppCompatActivity() {

    private lateinit var progressFromName: TextView
    private lateinit var progressToName: TextView
    private lateinit var progressOverallText: TextView
    private lateinit var progressSpeedText: TextView
    private lateinit var progressEtaText: TextView
    private lateinit var progressCancelBtn: MaterialButton
    private lateinit var progressFileList: RecyclerView
    private lateinit var progressOverallCircle: CircularProgressIndicator

    private lateinit var transferAdapter: TransferListAdapter

    private val progressReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            updateProgress()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_progress)

        progressFromName = findViewById(R.id.progressFromName)
        progressToName = findViewById(R.id.progressToName)
        progressOverallText = findViewById(R.id.progressOverallText)
        progressSpeedText = findViewById(R.id.progressSpeedText)
        progressEtaText = findViewById(R.id.progressEtaText)
        progressCancelBtn = findViewById(R.id.progressCancelBtn)
        progressFileList = findViewById(R.id.progressFileList)
        progressOverallCircle = findViewById(R.id.progressOverallCircle)

        progressFromName.text = intent.getStringExtra("FROM_NAME") ?: "Me"
        progressToName.text = intent.getStringExtra("TO_NAME") ?: "Device"

        transferAdapter = TransferListAdapter(
            onCancel = { id -> FileTransferManager.cancelTransfer(id); finish() },
            onRetry = null
        )
        progressFileList.layoutManager = LinearLayoutManager(this)
        progressFileList.adapter = transferAdapter
        progressFileList.isNestedScrollingEnabled = false

        progressCancelBtn.setOnClickListener {
            FileTransferManager.getActiveTransfers().forEach { FileTransferManager.cancelTransfer(it.id) }
            finish()
        }

        updateProgress()
    }

    override fun onResume() {
        super.onResume()
        LocalBroadcastManager.getInstance(this).registerReceiver(
            progressReceiver,
            IntentFilter(FileTransferManager.ACTION_PROGRESS)
        )
        updateProgress()
    }

    override fun onPause() {
        super.onPause()
        LocalBroadcastManager.getInstance(this).unregisterReceiver(progressReceiver)
    }

    private fun updateProgress() {
        val transfers = FileTransferManager.getActiveTransfers()
        transferAdapter.submitList(transfers)

        if (transfers.isEmpty()) {
            progressOverallText.text = "Complete"
            progressSpeedText.text = ""
            progressEtaText.text = ""
            progressCancelBtn.text = "Close"
            progressCancelBtn.setOnClickListener { finish() }
            progressOverallCircle.progress = 100
            return
        }

        val totalBytes = transfers.sumOf { it.totalBytes }
        val transferredBytes = transfers.sumOf { it.transferredBytes }
        val progress = if (totalBytes > 0) (transferredBytes * 100 / totalBytes).toInt() else 0

        progressOverallCircle.progress = progress
        progressOverallText.text = "${transfers.size} file${if (transfers.size > 1) "s" else ""} · $progress%"

        val sending = transfers.firstOrNull { it.status == TransferStatus.TRANSFERRING }
        if (sending != null) {
            progressSpeedText.text = FileTransferManager.formatSpeed(sending.instantSpeedBps)
            val remaining = sending.totalBytes - sending.transferredBytes
            if (sending.instantSpeedBps > 0) {
                val etaSecs = (remaining / sending.instantSpeedBps).toInt()
                if (etaSecs < 60) {
                    progressEtaText.text = "~${etaSecs}s remaining"
                } else {
                    progressEtaText.text = "~${etaSecs / 60}m ${etaSecs % 60}s remaining"
                }
            }
        }
    }
}
