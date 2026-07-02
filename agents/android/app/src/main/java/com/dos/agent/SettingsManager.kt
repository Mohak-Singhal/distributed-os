package com.dos.agent

import android.content.Context
import android.content.SharedPreferences
import android.net.Uri
import android.os.Environment
import java.io.File

object SettingsManager {
    private const val PREFS_NAME = "pdos_settings"
    private const val KEY_AUTO_ACCEPT = "auto_accept"
    private const val KEY_TRUSTED_PEERS = "trusted_peers"
    private const val KEY_DOWNLOAD_PATH = "download_path"
    private const val KEY_ENCRYPTION_ENABLED = "encryption_enabled"
    private const val KEY_DEVICE_NAME = "device_name"
    private const val KEY_SPEED_LIMIT_BPS = "speed_limit_bps"
    private const val KEY_ZIP_ON_RECEIVE = "zip_on_receive"

    private lateinit var prefs: SharedPreferences
    private var contextRef: Context? = null

    fun init(context: Context) {
        contextRef = context.applicationContext
        prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
    }

    var autoAccept: Boolean
        get() = prefs.getBoolean(KEY_AUTO_ACCEPT, false)
        set(value) = prefs.edit().putBoolean(KEY_AUTO_ACCEPT, value).apply()

    var encryptionEnabled: Boolean
        get() = prefs.getBoolean(KEY_ENCRYPTION_ENABLED, false)
        set(value) = prefs.edit().putBoolean(KEY_ENCRYPTION_ENABLED, value).apply()

    var deviceName: String
        get() = prefs.getString(KEY_DEVICE_NAME, android.os.Build.MODEL) ?: android.os.Build.MODEL
        set(value) = prefs.edit().putString(KEY_DEVICE_NAME, value).apply()

    var speedLimitBps: Long
        get() = prefs.getLong(KEY_SPEED_LIMIT_BPS, 0L)
        set(value) = prefs.edit().putLong(KEY_SPEED_LIMIT_BPS, value).apply()

    var zipOnReceive: Boolean
        get() = prefs.getBoolean(KEY_ZIP_ON_RECEIVE, false)
        set(value) = prefs.edit().putBoolean(KEY_ZIP_ON_RECEIVE, value).apply()

    fun getTrustedPeers(): Set<String> = prefs.getStringSet(KEY_TRUSTED_PEERS, emptySet()) ?: emptySet()

    fun addTrustedPeer(host: String) {
        val peers = getTrustedPeers().toMutableSet()
        peers.add(host)
        prefs.edit().putStringSet(KEY_TRUSTED_PEERS, peers).apply()
    }

    fun removeTrustedPeer(host: String) {
        val peers = getTrustedPeers().toMutableSet()
        peers.remove(host)
        prefs.edit().putStringSet(KEY_TRUSTED_PEERS, peers).apply()
    }

    fun isTrustedPeer(host: String): Boolean = getTrustedPeers().contains(host)

    fun getDownloadDir(): File {
        val path = prefs.getString(KEY_DOWNLOAD_PATH, null)
        if (path != null) {
            // Handle content:// URIs from SAF by extracting the filesystem path
            if (path.startsWith("content://")) {
                val decoded = android.net.Uri.decode(path)
                // Extract path after /tree/primary: and before /document/
                val treeIdx = decoded.indexOf("tree/primary%3A")
                    ?: decoded.indexOf("tree/primary:")
                if (treeIdx >= 0) {
                    var extracted = decoded.substring(treeIdx + if (decoded[treeIdx + 5] == '%') 18 else 13)
                    val docIdx = extracted.indexOf("/document/")
                    if (docIdx >= 0) extracted = extracted.substring(0, docIdx)
                    val dir = File(Environment.getExternalStorageDirectory(), extracted)
                    if (dir.exists() || dir.mkdirs()) return dir
                }
            } else {
                val dir = File(path)
                if (dir.exists() || dir.mkdirs()) return dir
            }
        }
        return Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)
    }

    fun setDownloadDir(uri: Uri) {
        prefs.edit().putString(KEY_DOWNLOAD_PATH, uri.toString()).apply()
    }
}
