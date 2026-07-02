package com.dos.agent

/**
 * Data model representing a discovered peer device.
 * Pure data class with no business logic — parsing is in DeviceNameResolver.
 */
data class PeerItem(
    val name: String,
    val host: String,
    val port: Int,
    val platform: String,
    val isSelected: Boolean = false,
    val isTrusted: Boolean = false
) {
    /** Human-readable device name (e.g., "Samsung S23" instead of raw mDNS name) */
    val displayName: String
        get() = DeviceNameResolver.resolveDisplayName(name, platform, host)

    /** Emoji icon representing the device type */
    val deviceEmoji: String
        get() = PlatformMapper.deviceEmoji(name, platform)

    /** Human-readable platform label (e.g., "Samsung", "Google Pixel") */
    val platformLabel: String
        get() = PlatformMapper.platformLabel(name, platform)
}
