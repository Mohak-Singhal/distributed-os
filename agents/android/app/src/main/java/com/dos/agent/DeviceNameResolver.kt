package com.dos.agent

/**
 * Resolves human-readable device names from raw mDNS service names and platform info.
 * Centralizes all name-parsing heuristics in one place.
 */
object DeviceNameResolver {

    /**
     * Extracts a friendly display name from the raw mDNS service name.
     *
     * Examples:
     *   "PDOS-Android-Samsung S23" → "Samsung S23"
     *   "PDOS-Mac-MOHAKs-MacBook-Air" → "MOHAKs-MacBook-Air"
     *   "PDOS-Android" → "Android (192.168.1.11)"
     */
    fun resolveDisplayName(rawName: String, platform: String, host: String): String {
        val cleaned = rawName
            .removePrefix("PDOS-Android-")
            .removePrefix("PDOS-Mac-")
            .removePrefix("PDOS-")
            .trim()

        return when {
            cleaned.isNotEmpty() && cleaned != "Android" && cleaned != "Mac" -> cleaned
            platform == "mac" -> "Mac (${host.takeLast(7)})"
            else -> "Android (${host.takeLast(7)})"
        }
    }
}
