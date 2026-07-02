package com.dos.agent

import android.app.Application

class PdosApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        SettingsManager.init(this)
        FileTransferManager.init(this)
    }
}
