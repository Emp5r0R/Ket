package com.ket.android

import org.junit.Assert.assertEquals
import org.junit.Test

class AndroidTransportHistoryTest {
    @Test
    fun `cooling transport falls behind a healthy fallback`() {
        val history = AndroidTransportHistory(baseCooldownMillis = 1_000, maximumCooldownMillis = 8_000)
        val primary = TestTransport("primary", priority = 5)
        val fallback = TestTransport("fallback", priority = 10)

        assertEquals(listOf("primary", "fallback"), history.rank(listOf(fallback, primary), 100).map { it.id })

        history.recordFailure(primary.id, nowMillis = 100)

        assertEquals(listOf("fallback", "primary"), history.rank(listOf(primary, fallback), 1_099).map { it.id })
        assertEquals(listOf("fallback", "primary"), history.rank(listOf(primary, fallback), 1_100).map { it.id })

        history.recordFailure(fallback.id, nowMillis = 1_100)

        assertEquals(listOf("primary", "fallback"), history.rank(listOf(primary, fallback), 1_101).map { it.id })
    }

    @Test
    fun `success clears failure penalty and retains measured latency`() {
        val history = AndroidTransportHistory(baseCooldownMillis = 1_000, maximumCooldownMillis = 8_000)
        val first = TestTransport("first", priority = 5)
        val second = TestTransport("second", priority = 5)
        history.recordFailure(first.id, nowMillis = 0)
        history.recordSuccess(first.id, latencyMillis = 30)
        history.recordSuccess(second.id, latencyMillis = 90)

        assertEquals(listOf("first", "second"), history.rank(listOf(second, first), 1).map { it.id })
    }

    @Test
    fun `repeated failures apply bounded exponential cooldown`() {
        val history = AndroidTransportHistory(baseCooldownMillis = 1_000, maximumCooldownMillis = 4_000)
        val primary = TestTransport("primary", priority = 5)
        val fallback = TestTransport("fallback", priority = 10)

        history.recordFailure(primary.id, nowMillis = 0)
        history.recordFailure(primary.id, nowMillis = 1_000)
        history.recordFailure(primary.id, nowMillis = 3_000)
        history.recordFailure(primary.id, nowMillis = 7_000)
        repeat(5) { history.recordFailure(fallback.id, nowMillis = 0) }

        assertEquals("fallback", history.rank(listOf(primary, fallback), 10_999).first().id)
        assertEquals("primary", history.rank(listOf(primary, fallback), 11_000).first().id)
    }

    @Test
    fun `lease failures reserve a recovery window before shutdown`() {
        val policy = AndroidRecoveryPolicy(
            maximumReconnectRounds = 3,
            recoverAfterFailures = 2,
            stopAfterFailures = 5,
        )

        assertEquals(AndroidLeaseFailureAction.Wait, policy.leaseFailureAction(1))
        assertEquals(AndroidLeaseFailureAction.Recover, policy.leaseFailureAction(2))
        assertEquals(AndroidLeaseFailureAction.Wait, policy.leaseFailureAction(3))
        assertEquals(AndroidLeaseFailureAction.Wait, policy.leaseFailureAction(4))
        assertEquals(AndroidLeaseFailureAction.Stop, policy.leaseFailureAction(5))
        assertEquals(AndroidLeaseFailureAction.Stop, policy.leaseFailureAction(8))
    }

    @Test
    fun `authorization loss stops without futile transport recovery`() {
        val policy = AndroidRecoveryPolicy()

        assertEquals(
            AndroidLeaseFailureAction.Stop,
            policy.leaseFailureAction(consecutiveFailures = 1, authorizationLost = true),
        )
    }

    private data class TestTransport(
        override val id: String,
        override val priority: Int,
    ) : AndroidTransport {
        override val endpoint: String = "vpn.example.test"
        override val port: Int = 443
        override val displayName: String = id
    }
}
