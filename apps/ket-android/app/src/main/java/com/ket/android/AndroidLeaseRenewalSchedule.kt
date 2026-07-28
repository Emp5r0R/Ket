package com.ket.android

/** Schedules one renewal around the midpoint of each confirmed server lease. */
internal object AndroidLeaseRenewalSchedule {
    fun nextRenewalEpochSeconds(nowEpochSeconds: Long, expiresAtEpochSeconds: Long): Long {
        require(nowEpochSeconds >= 0) { "Current epoch time must not be negative" }
        require(expiresAtEpochSeconds > nowEpochSeconds) { "Session lease must be active" }
        val remaining = expiresAtEpochSeconds - nowEpochSeconds
        return nowEpochSeconds + (remaining / 2).coerceAtLeast(1)
    }

    fun isDue(nowEpochSeconds: Long, renewAtEpochSeconds: Long): Boolean =
        nowEpochSeconds >= renewAtEpochSeconds
}
