package com.dos.agent.pairing

/** Result of a QR pairing handshake with a Mac device. */
sealed class PairResult {
    data class Success(
        val relayUrl: String,
        val nodeId: String,
        val nodeName: String
    ) : PairResult()

    data class Error(val message: String) : PairResult()
}
