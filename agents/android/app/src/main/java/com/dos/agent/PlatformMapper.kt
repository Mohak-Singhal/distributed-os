package com.dos.agent

/**
 * Centralized platform-to-visual mapping.
 * Single source of truth for device emojis, icons, and labels across the app.
 */
object PlatformMapper {

    private val androidBrands = listOf(
        "Pixel", "Samsung", "Galaxy", "OnePlus", "Xiaomi",
        "Oppo", "Vivo", "Nothing", "Motorola", "Nokia", "Sony"
    )

    /** Device emoji based on platform and name heuristics */
    fun deviceEmoji(name: String, platform: String): String {
        return when {
            platform == "mac" || name.contains("Mac", ignoreCase = true) -> "\uD83D\uDDA5\uFE0F" // 🖥️
            platform == "android" || name.contains("Android", ignoreCase = true) -> "\uD83D\uDCF1" // 📱
            platform == "linux" -> "\uD83D\uDDA5\uFE0F" // 🖥️
            platform == "windows" -> "\uD83D\uDDA5\uFE0F" // 🖥️
            else -> "\uD83D\uDDA5\uFE0F" // 🖥️
        }
    }

    /** Human-readable platform label with brand detection */
    fun platformLabel(name: String, platform: String): String {
        val brand = androidBrands.firstOrNull { name.contains(it, ignoreCase = true) }
        if (brand != null) return brand

        return when (platform.lowercase()) {
            "macos", "mac" -> "macOS"
            "android" -> "Android"
            "linux" -> "Linux"
            "windows" -> "Windows"
            else -> platform.replaceFirstChar { it.uppercase() }
        }
    }

    /** Platform icon resource name (for XML drawables) */
    fun platformIconRes(platform: String): Int {
        return when (platform.lowercase()) {
            "macos", "mac" -> R.drawable.ic_device_laptop
            "android" -> R.drawable.ic_device_phone
            else -> R.drawable.ic_device_desktop
        }
    }
}
