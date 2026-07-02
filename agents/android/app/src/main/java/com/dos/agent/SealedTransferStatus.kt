package com.dos.agent

sealed class SealedTransferStatus {
    object Idle : SealedTransferStatus()
    data class Sending(val progress: Float, val speed: String, val eta: String = "") : SealedTransferStatus()
    object Success : SealedTransferStatus()
    data class Failed(val error: String) : SealedTransferStatus()

    val message: String
        get() = when (this) {
            is Idle -> ""
            is Sending -> "Sending — $speed"
            is Success -> "Complete"
            is Failed -> error
        }

    val isError: Boolean
        get() = this is Failed

    companion object {
        fun from(state: TransferState): SealedTransferStatus {
            return when (state.status) {
                TransferStatus.QUEUED, TransferStatus.CONNECTING -> Sending(0f, "Starting...")
                TransferStatus.TRANSFERRING -> {
                    val speed = FileTransferManager.formatSpeed(state.instantSpeedBps)
                    val eta = if (state.instantSpeedBps > 0 && state.totalBytes > 0) {
                        val remaining = state.totalBytes - state.transferredBytes
                        val seconds = (remaining / state.instantSpeedBps).toInt()
                        if (seconds > 60) "${seconds / 60}m ${seconds % 60}s" else "${seconds}s"
                    } else ""
                    Sending(state.progressPercent / 100f, speed, eta)
                }
                TransferStatus.COMPLETED -> Success
                TransferStatus.FAILED -> Failed(state.errorMessage.ifEmpty { "Transfer failed" })
                TransferStatus.CANCELLED -> Failed("Cancelled")
                TransferStatus.PAUSED -> Sending(state.progressPercent / 100f, "Paused")
            }
        }
    }
}
