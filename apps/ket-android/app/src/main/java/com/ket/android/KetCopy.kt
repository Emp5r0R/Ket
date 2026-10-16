package com.ket.android

/** The words the app shows for each connection state, kept apart from layout so they are tested. */

internal fun phaseStatusLabel(phase: TunnelPhase): String = when (phase) {
    TunnelPhase.Disconnected -> "You are being watched"
    TunnelPhase.Enrolling -> "Authorizing"
    TunnelPhase.Connecting -> "Connecting"
    TunnelPhase.Reconnecting -> "Recovering"
    TunnelPhase.Connected -> "Liberated"
    TunnelPhase.Stopping -> "Stopping"
    TunnelPhase.Failed -> "Attention"
}

internal fun phaseHeadline(phase: TunnelPhase): String = when (phase) {
    TunnelPhase.Connected -> "Liberated route"
    TunnelPhase.Enrolling, TunnelPhase.Connecting -> "Securing route"
    TunnelPhase.Reconnecting -> "Restoring route"
    TunnelPhase.Stopping -> "Closing route"
    TunnelPhase.Failed -> "Route needs attention"
    TunnelPhase.Disconnected -> "Connect to get liberated"
}

internal fun disconnectedSurveillanceLabel(variant: Int): String =
    if (variant % 2 == 0) "You are being watched" else "You are under surveillance"

/** The server's place, naming a city-state such as Singapore once. */
internal fun connectionLocationHeading(node: AndroidNodeStatus?): String = node?.location?.let { location ->
    val city = location.city?.takeUnless { it.isBlank() || it.equals(location.countryName, ignoreCase = true) }
    if (city == null) location.countryName else "$city, ${location.countryName}"
} ?: "Your server"

internal fun connectionActionLabel(phase: TunnelPhase): String = when (phase) {
    TunnelPhase.Connected -> "Disconnect tunnel"
    TunnelPhase.Enrolling, TunnelPhase.Connecting, TunnelPhase.Reconnecting -> "Cancel connection attempt"
    TunnelPhase.Stopping -> "Disconnecting tunnel"
    TunnelPhase.Disconnected, TunnelPhase.Failed -> "Connect tunnel"
}

internal fun connectionPowerHeadline(phase: TunnelPhase): String = when (phase) {
    TunnelPhase.Connected -> "Liberated"
    TunnelPhase.Enrolling -> "Adding server"
    TunnelPhase.Connecting -> "Connecting"
    TunnelPhase.Reconnecting -> "Reconnecting"
    TunnelPhase.Stopping -> "Disconnecting"
    TunnelPhase.Failed -> "Connection failed"
    TunnelPhase.Disconnected -> "Connect to get liberated"
}

internal fun connectionPowerDetail(phase: TunnelPhase): String = when (phase) {
    TunnelPhase.Connected -> "Protected route active"
    TunnelPhase.Enrolling, TunnelPhase.Connecting, TunnelPhase.Reconnecting -> "Testing available routes"
    TunnelPhase.Stopping -> "Stopping tunnel and restoring network"
    TunnelPhase.Failed -> "Open diagnostics, then try again"
    TunnelPhase.Disconnected -> "You are under surveillance"
}

internal fun formatBytes(bytes: Long): String {
    if (bytes < 1024) return "$bytes B"
    val units = arrayOf("KB", "MB", "GB", "TB")
    var value = bytes.toDouble()
    var unit = -1
    while (value >= 1024 && unit < units.lastIndex) {
        value /= 1024
        unit += 1
    }
    return "%.1f %s".format(value, units[unit])
}

internal fun formatDuration(seconds: Long?): String {
    if (seconds == null) return "--"
    val days = seconds / 86_400
    val hours = seconds % 86_400 / 3_600
    val minutes = seconds % 3_600 / 60
    return when {
        days > 0 -> "${days}d ${hours}h"
        hours > 0 -> "${hours}h ${minutes}m"
        else -> "${minutes}m"
    }
}

/** How long the route took to come up; it includes engine start, so it is not a ping. */
internal fun formatSetupTime(milliseconds: Long?): String = when {
    milliseconds == null -> "--"
    milliseconds < 1_000 -> "$milliseconds ms"
    else -> "%.1f s".format(milliseconds / 1_000.0)
}

/** The text that is a Ket invite inside anything pasted or shared, or null. */
internal fun extractInvite(text: String?): String? =
    text?.let { INVITE_PATTERN.find(it)?.value }

private val INVITE_PATTERN = Regex("ket:invite:[A-Za-z0-9_-]+")
