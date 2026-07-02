package com.dos.agent.pairing

import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/**
 * Makes an HTTP GET request to the Mac's pairing server to validate
 * a one-time token and retrieve device identity.
 */
class PairingClient {

    /**
     * Sends the pairing request on a background thread.
     * @param host Mac's IP address.
     * @param port The ephemeral port the pairing server is listening on.
     * @param token The 64-char hex token from the QR code.
     * @param callback Invoked on completion with either [PairResult.Success] or [PairResult.Error].
     */
    fun pair(host: String, port: Int, token: String, callback: (PairResult) -> Unit) {
        Thread {
            try {
                val url = URL("http://$host:$port/pair?token=$token")
                val connection = url.openConnection() as HttpURLConnection
                connection.connectTimeout = 10000
                connection.readTimeout = 10000
                connection.requestMethod = "GET"

                val responseCode = connection.responseCode
                if (responseCode == 200) {
                    val body = connection.inputStream.bufferedReader().use { it.readText() }
                    val json = JSONObject(body)
                    val accepted = json.optBoolean("accepted", false)

                    if (accepted) {
                        callback(PairResult.Success(
                            relayUrl = json.optString("relay_url", "ws://$host:7890"),
                            nodeId = json.optString("node_id", ""),
                            nodeName = json.optString("node_name", host)
                        ))
                    } else {
                        callback(PairResult.Error(json.optString("error", "Pairing rejected")))
                    }
                } else {
                    val error = connection.errorStream?.bufferedReader()?.readText()
                        ?: "HTTP $responseCode"
                    callback(PairResult.Error(error))
                }
                connection.disconnect()
            } catch (e: Exception) {
                callback(PairResult.Error(e.message ?: "Connection failed"))
            }
        }.start()
    }
}
