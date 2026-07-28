package com.ket.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidLeaseRenewalScheduleTest {
    @Test
    fun `ordinary telemetry intervals do not renew before lease midpoint`() {
        val renewAt = AndroidLeaseRenewalSchedule.nextRenewalEpochSeconds(
            nowEpochSeconds = 1_000,
            expiresAtEpochSeconds = 1_090,
        )

        assertEquals(1_045, renewAt)
        listOf(1_000L, 1_010L, 1_020L, 1_030L, 1_040L).forEach {
            assertFalse(AndroidLeaseRenewalSchedule.isDue(it, renewAt))
        }
        assertTrue(AndroidLeaseRenewalSchedule.isDue(1_050, renewAt))
    }

    @Test
    fun `refreshed expiry creates a new midpoint instead of renewing every poll`() {
        val firstRenewal = AndroidLeaseRenewalSchedule.nextRenewalEpochSeconds(1_000, 1_090)
        val nextRenewal = AndroidLeaseRenewalSchedule.nextRenewalEpochSeconds(1_050, 1_140)

        assertEquals(1_045, firstRenewal)
        assertEquals(1_095, nextRenewal)
        assertFalse(AndroidLeaseRenewalSchedule.isDue(1_090, nextRenewal))
    }

    @Test
    fun `invalid or expired leases are rejected`() {
        assertThrows(IllegalArgumentException::class.java) {
            AndroidLeaseRenewalSchedule.nextRenewalEpochSeconds(1_000, 1_000)
        }
        assertThrows(IllegalArgumentException::class.java) {
            AndroidLeaseRenewalSchedule.nextRenewalEpochSeconds(-1, 1_000)
        }
    }
}
