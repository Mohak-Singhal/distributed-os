package com.dos.agent

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.Build

class NetworkMonitor(private val context: Context) {

    private val connectivityManager =
        context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager

    private var callback: ConnectivityManager.NetworkCallback? = null
    private var legacyReceiver: BroadcastReceiver? = null
    private var onWifiStateChanged: ((Boolean) -> Unit)? = null

    fun start(onChange: (Boolean) -> Unit) {
        onWifiStateChanged = onChange

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
            val request = NetworkRequest.Builder()
                .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
                .build()
            callback = object : ConnectivityManager.NetworkCallback() {
                override fun onAvailable(network: Network) {
                    onChange(true)
                }
                override fun onLost(network: Network) {
                    onChange(false)
                }
                override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) {
                    onChange(caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI))
                }
            }
            connectivityManager.registerNetworkCallback(request, callback!!)
        } else {
            legacyReceiver = object : BroadcastReceiver() {
                override fun onReceive(ctx: Context, intent: Intent) {
                    val info = connectivityManager.activeNetworkInfo
                    onChange(info != null && info.type == ConnectivityManager.TYPE_WIFI)
                }
            }
            context.registerReceiver(legacyReceiver, IntentFilter(ConnectivityManager.CONNECTIVITY_ACTION))
        }
    }

    fun stop() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
            callback?.let { connectivityManager.unregisterNetworkCallback(it) }
            callback = null
        }
        legacyReceiver?.let { context.unregisterReceiver(it) }
        legacyReceiver = null
    }
}
