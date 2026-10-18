package com.ket.android

import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.TimeZone
import org.json.JSONObject

/** How a data allowance is counted. Periods follow the UTC calendar on the server. */
enum class AllowancePeriod(val wireName: String, val each: String, val current: String) {
    Day("day", "a day", "today"),
    Week("week", "a week", "this week"),
    Month("month", "a month", "this month"),
}

/** The invite's data allowance, shared by all its devices, and this period's use. */
data class DataAllowanceStatus(
    val bytes: Long,
    val period: AllowancePeriod,
    val usedBytes: Long,
    val resetsAtEpochSeconds: Long,
) {
    val usedUp: Boolean get() = usedBytes >= bytes
    val fraction: Float get() = (usedBytes.toDouble() / bytes).toFloat().coerceIn(0f, 1f)
}

internal object DataAllowanceParser {
    private const val MIN_BYTES = 1L shl 20
    private const val MAX_BYTES = 1L shl 50

    /**
     * The `allowance` of a session status, or null when the server sets none. A period this
     * app does not know is left out rather than failing the whole status.
     */
    fun parse(json: JSONObject?): DataAllowanceStatus? {
        if (json == null) return null
        val period = AllowancePeriod.entries.firstOrNull { it.wireName == json.getString("period") }
            ?: return null
        val bytes = json.getLong("bytes")
        require(bytes in MIN_BYTES..MAX_BYTES) { "Data allowance is invalid" }
        val used = json.getLong("used_bytes")
        require(used >= 0) { "Data allowance use is invalid" }
        val resetsAt = json.getLong("resets_at_epoch_seconds")
        require(resetsAt > 0) { "Data allowance reset time is invalid" }
        return DataAllowanceStatus(bytes, period, used, resetsAt)
    }
}

/** "1 Nov 2026, 00:00 UTC": when a used-up allowance starts again. */
internal fun formatAllowanceReset(epochSeconds: Long): String =
    SimpleDateFormat("d MMM yyyy, HH:mm", Locale.UK)
        .apply { timeZone = TimeZone.getTimeZone("UTC") }
        .format(Date(epochSeconds * 1_000)) + " UTC"
