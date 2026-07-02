package com.dos.agent

object NativeTransferEngine {
    init {
        // Load the shared library containing the high-performance Rust core
        System.loadLibrary("dos_android")
    }

    /**
     * Starts the highly optimized Rust Tokio HTTP server directly inside the NDK.
     * This handles file transfers with zero-copy disk writes and completely bypasses
     * the Kotlin JVM overhead.
     *
     * @param port The port to bind the server to (e.g. 7894)
     * @param downloadDir The absolute path where received files should be stored.
     */
    external fun startServer(port: Int, downloadDir: String)
}
