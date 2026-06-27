package com.dos.agent

interface NodeCallback {
    fun onStateChanged(stateJson: String)
    fun onLog(level: Int, message: String)
}

object Core {
    init {
        System.loadLibrary("dos_android")
    }

    /**
     * Starts the Rust Tokio runtime in a background native thread.
     * @param configPath The absolute path to the dos-config.toml file.
     * @param callback The interface for Rust to call back into Kotlin.
     */
    external fun startAgent(configPath: String, callback: NodeCallback)

    /**
     * Signals the Rust Tokio runtime to shut down.
     */
    external fun stopAgent()
}
