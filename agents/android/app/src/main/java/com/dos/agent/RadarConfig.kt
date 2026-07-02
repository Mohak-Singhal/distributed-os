package com.dos.agent

/**
 * Configuration constants for the radar scanner and device discovery UI.
 * All magic numbers and configurable values live here.
 */
object RadarConfig {

    // ── Radar Animation ──────────────────────────────────────
    const val sweepDurationMs = 3000L
    const val pulseDurationMs = 1800L
    const val ringCount = 3
    const val ringBaseRadiusFraction = 0.3f
    const val ringSpacingFraction = 0.25f
    const val sweepConeAngle = 15f
    const val centerDotRadiusDp = 4f
    const val orbitingDotRadiusDp = 3f
    const val orbitingDotCount = 3
    const val orbitRadiusFraction = 0.6f

    // ── Radar Colors ─────────────────────────────────────────
    const val radarColor = 0xFF00E5FF.toInt()
    const val radarConeColor = 0x1800E5FF.toInt()

    // ── Device List ──────────────────────────────────────────
    const val deviceListMaxHeightPercent = 0.35f
    const val deviceCardEntranceStaggerMs = 60L

    // ── UI Status Strings ────────────────────────────────────
    const val STATUS_OFFLINE = "STATUS: OFFLINE"
    const val STATUS_SEARCHING = "STATUS: SEARCHING mDNS..."
    const val STATUS_LINK_ESTABLISHED = "STATUS: LINK ESTABLISHED"
    const val STATUS_FAILED = "STATUS: FAILED"

    // ── UI Colors ────────────────────────────────────────────
    const val COLOR_ONLINE = "#FFFFFF"
    const val COLOR_OFFLINE = "#888888"
    const val COLOR_SEARCHING = "#FFFFFF"

    // ── Timeouts ─────────────────────────────────────────────
    const val errorResetDelayMs = 3000L
    const val filePickerMime = "*/*"
}
