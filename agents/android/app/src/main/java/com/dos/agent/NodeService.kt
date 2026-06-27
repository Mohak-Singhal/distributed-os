package com.dos.agent

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.io.File
import java.io.FileWriter

class NodeService : Service() {

    companion object {
        const val ACTION_START = "ACTION_START"
        const val ACTION_STOP = "ACTION_STOP"
        private const val CHANNEL_ID = "pdos_node_channel"

        // State flows to expose node state to the UI
        private val _nodeState = MutableStateFlow("Offline")
        val nodeState = _nodeState.asStateFlow()
        
        private val _logs = MutableStateFlow(listOf<String>())
        val logs = _logs.asStateFlow()
    }

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> startNode()
            ACTION_STOP -> stopNode()
        }
        return START_STICKY
    }

    private fun startNode() {
        val notification = NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("PDOS Node")
            .setContentText("Node is running in the background")
            .setSmallIcon(android.R.drawable.ic_menu_preferences)
            .build()
            
        startForeground(1, notification)
        _nodeState.value = "Starting..."

        val configPath = File(filesDir, "dos-config.toml").absolutePath
        val configFile = File(configPath)
        
        // Force update to the Mac's physical IP address on Wi-Fi for testing
        FileWriter(configFile).use { writer ->
            writer.write("relay_url = \"ws://192.168.1.3:7890\"\n")
        }

        Core.startAgent(configPath, object : NodeCallback {
            override fun onStateChanged(stateJson: String) {
                _nodeState.value = stateJson
                addLog("State changed: $stateJson")
            }
            override fun onLog(level: Int, message: String) {
                addLog("[$level] $message")
            }
        })
    }

    private fun stopNode() {
        Core.stopAgent()
        _nodeState.value = "Offline"
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun addLog(message: String) {
        val current = _logs.value.toMutableList()
        current.add(message)
        if (current.size > 100) current.removeAt(0)
        _logs.value = current
    }

    override fun onBind(intent: Intent?): IBinder? {
        return null
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val serviceChannel = NotificationChannel(
                CHANNEL_ID,
                "PDOS Node Service",
                NotificationManager.IMPORTANCE_LOW
            )
            val manager = getSystemService(NotificationManager::class.java)
            manager?.createNotificationChannel(serviceChannel)
        }
    }
}
