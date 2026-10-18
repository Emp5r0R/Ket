package com.ket.android.ui.metrics

import com.ket.android.formatBytes

/** Running traffic totals read at [atMillis] (elapsed realtime). */
data class TrafficSample(val atMillis: Long, val sent: Long, val received: Long)

/** Bytes per second in each direction over the interval ending at [atMillis]. */
data class TrafficRate(val atMillis: Long, val sent: Double, val received: Double)

/**
 * Speeds from running traffic totals. A total that went down belongs to a new connection, so
 * what it holds counts as moved in that interval.
 */
object Throughput {
    /** About 80 seconds of samples taken every two seconds. */
    const val HISTORY = 40

    /** [history] with [sample] added, unless it repeats the last reading. */
    fun append(history: List<TrafficSample>, sample: TrafficSample): List<TrafficSample> {
        val last = history.lastOrNull()
        if (last != null && sample.atMillis <= last.atMillis) return history
        return (history + sample).takeLast(HISTORY)
    }

    fun rates(history: List<TrafficSample>): List<TrafficRate> = history.zipWithNext { before, after ->
        val seconds = (after.atMillis - before.atMillis) / 1_000.0
        TrafficRate(
            atMillis = after.atMillis,
            sent = moved(before.sent, after.sent) / seconds,
            received = moved(before.received, after.received) / seconds,
        )
    }

    /** The most recent speed, or null before there are two samples. */
    fun latest(history: List<TrafficSample>): TrafficRate? = rates(history.takeLast(2)).lastOrNull()

    private fun moved(before: Long, after: Long): Long = if (after >= before) after - before else after
}

internal fun formatRate(bytesPerSecond: Double?): String =
    if (bytesPerSecond == null || !bytesPerSecond.isFinite()) "--" else "${formatBytes(bytesPerSecond.toLong())}/s"
