package com.dos.agent

import android.content.Intent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import androidx.annotation.RequiresApi

@RequiresApi(Build.VERSION_CODES.N)
class PdosTileService : TileService() {

    override fun onStartListening() {
        super.onStartListening()
        updateTileState()
    }

    override fun onClick() {
        super.onClick()
        toggleService()
    }

    private fun toggleService() {
        if (isServiceRunning()) {
            stopPdosService()
        } else {
            startPdosService()
        }
        updateTileState()
    }

    private fun isServiceRunning(): Boolean {
        return NodeService.nodeState.value != "Offline"
    }

    private fun startPdosService() {
        val intent = createServiceIntent(NodeService.ACTION_START)
        startServiceSafely(intent)
    }

    private fun stopPdosService() {
        val intent = createServiceIntent(NodeService.ACTION_STOP)
        startServiceSafely(intent)
    }

    private fun createServiceIntent(action: String): Intent {
        return Intent(this, NodeService::class.java).apply {
            this.action = action
            if (action == NodeService.ACTION_START) {
                putExtra("RELAY_IP", "discover")
            }
        }
    }

    private fun startServiceSafely(intent: Intent) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(intent)
        } else {
            startService(intent)
        }
    }

    private fun updateTileState() {
        val tile = qsTile ?: return
        val isActive = isServiceRunning()

        tile.apply {
            state = if (isActive) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE
            label = if (isActive) "PDOS Active" else "PDOS"
            contentDescription = if (isActive) "PDOS receiver is active" else "PDOS receiver is off"

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                stateDescription = if (isActive) "Active" else "Inactive"
            }
        }

        tile.updateTile()
    }
}
