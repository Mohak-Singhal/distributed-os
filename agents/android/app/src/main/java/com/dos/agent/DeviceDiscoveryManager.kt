package com.dos.agent

class DeviceDiscoveryManager(
    private val onPeersChanged: (List<PeerItem>) -> Unit
) {
    private val peers = mutableListOf<PeerItem>()

    val discoveredPeers: List<PeerItem> get() = peers.toList()
    val deviceCount: Int get() = peers.size

    fun addOrUpdate(name: String, host: String, port: Int, platform: String) {
        val existing = peers.indexOfFirst { it.host == host }
        val peer = PeerItem(
            name = name, host = host, port = port, platform = platform,
            isSelected = false,
            isTrusted = SettingsManager.isTrustedPeer(host)
        )
        if (existing >= 0) peers[existing] = peer
        else peers.add(peer)
        onPeersChanged(peers.toList())
    }

    fun remove(host: String) {
        peers.removeAll { it.host == host }
        onPeersChanged(peers.toList())
    }

    fun clear() {
        peers.clear()
        onPeersChanged(peers.toList())
    }

    fun isEmpty(): Boolean = peers.isEmpty()
}
