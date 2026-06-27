package com.dos.agent

import android.content.Intent
import android.os.Bundle
import android.widget.Button
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.launch

class MainActivity : AppCompatActivity() {

    private lateinit var statusText: TextView
    private lateinit var logsText: TextView
    private lateinit var btnStart: Button
    private lateinit var btnStop: Button

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Programmatically create simple UI to avoid XML overhead for now
        val layout = android.widget.LinearLayout(this).apply {
            orientation = android.widget.LinearLayout.VERTICAL
            setPadding(32, 32, 32, 32)
        }

        statusText = TextView(this).apply {
            text = "Status: Offline"
            textSize = 20f
        }
        
        btnStart = Button(this).apply {
            text = "Start Node"
            setOnClickListener {
                val intent = Intent(this@MainActivity, NodeService::class.java)
                intent.action = NodeService.ACTION_START
                startService(intent)
            }
        }

        btnStop = Button(this).apply {
            text = "Stop Node"
            setOnClickListener {
                val intent = Intent(this@MainActivity, NodeService::class.java)
                intent.action = NodeService.ACTION_STOP
                startService(intent)
            }
        }

        logsText = TextView(this).apply {
            text = "Logs:\n"
            textSize = 12f
        }

        layout.addView(statusText)
        layout.addView(btnStart)
        layout.addView(btnStop)
        layout.addView(logsText)
        
        setContentView(layout)

        // Observe StateFlow from Service
        lifecycleScope.launch {
            NodeService.nodeState.collect { state ->
                statusText.text = "Status: $state"
            }
        }

        lifecycleScope.launch {
            NodeService.logs.collect { logs ->
                logsText.text = logs.takeLast(10).joinToString("\n")
            }
        }
    }
}
