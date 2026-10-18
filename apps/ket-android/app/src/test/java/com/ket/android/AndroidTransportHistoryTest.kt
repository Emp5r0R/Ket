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
    fun `explicit protocol selection excludes every other protocol`() {
        val history = AndroidTransportHistory(baseCooldownMillis = 1_000, maximumCooldownMillis = 8_000)
        val primary = TestTransport("reality", priority = 1, protocol = KetProtocol.Reality)
        val preferred = TestTransport("hysteria-primary", priority = 10, protocol = KetProtocol.Hysteria2)
        val preferredBackup = TestTransport("hysteria-secondary", priority = 20, protocol = KetProtocol.Hysteria2)

        assertEquals(
            listOf("hysteria-primary", "hysteria-secondary"),
            history.rank(listOf(primary, preferredBackup, preferred), 0, KetProtocol.Hysteria2).map { it.id },
        )

        history.recordFailure(preferred.id, nowMillis = 0)
        assertEquals(
            listOf("hysteria-secondary", "hysteria-primary"),
            history.rank(listOf(primary, preferred, preferredBackup), 500, KetProtocol.Hysteria2).map { it.id },
        )
    }

    @Test
    fun `a transport that worked here goes before an untried one of higher priority`() {
        val history = AndroidTransportHistory()
        val untried = TestTransport("untried", priority = 1)
        val worked = TestTransport("worked", priority = 9)
        history.recordSuccess(worked.id, latencyMillis = 80)
        assertEquals(listOf("worked", "untried"), history.rank(listOf(untried, worked), 0).map { it.id })
    }

    @Test
    fun `automatic leaves out unchecked protocols unless nothing would remain`() {
        val history = AndroidTransportHistory()
        val reality = TestTransport("reality", priority = 1, protocol = KetProtocol.Reality)
        val hysteria = TestTransport("hysteria", priority = 2, protocol = KetProtocol.Hysteria2)
        val transports = listOf(reality, hysteria)
        assertEquals(
            listOf("hysteria"),
            history.rank(transports, 0, automaticExcluded = setOf(KetProtocol.Reality)).map { it.id },
        )
        assertEquals(
            listOf("reality"),
            history.rank(transports, 0, KetProtocol.Reality, setOf(KetProtocol.Reality)).map { it.id },
        )
        assertEquals(
            listOf("reality", "hysteria"),
            history.rank(transports, 0, automaticExcluded = setOf(KetProtocol.Reality, KetProtocol.Hysteria2))
                .map { it.id },
        )
    }

    @Test
    fun `saved outcomes come back without uptime-based cooldowns`() {
        val history = AndroidTransportHistory(baseCooldownMillis = 1_000, maximumCooldownMillis = 8_000)
        history.recordSuccess("worked", latencyMillis = 40)
        history.recordFailure("failed", nowMillis = 0)
        val restored = AndroidTransportHistory(baseCooldownMillis = 1_000, maximumCooldownMillis = 8_000)
        restored.restore(org.json.JSONObject(history.export().toString()))
        val worked = TestTransport("worked", priority = 9)
        val failed = TestTransport("failed", priority = 1)
        val untried = TestTransport("untried", priority = 5)
        assertEquals(
            "the failure is remembered but its cooldown is not",
            listOf("worked", "untried", "failed"),
            restored.rank(listOf(failed, untried, worked), 0).map { it.id },
        )
        restored.restore(null)
        assertEquals(listOf("failed", "untried", "worked"), restored.rank(listOf(failed, untried, worked), 0).map { it.id })
    }

    @Test
    fun `explicit unavailable protocol returns no candidates`() {
        val history = AndroidTransportHistory()
        val reality = TestTransport("reality", priority = 1, protocol = KetProtocol.Reality)

        assertEquals(emptyList<AndroidTransport>(), history.rank(listOf(reality), 0, KetProtocol.Stealth))
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
    fun `lease failures recover only after the routed path also fails`() {
        val policy = AndroidRecoveryPolicy(
            maximumReconnectRounds = 3,
            recoverAfterFailures = 2,
            stopAfterFailures = 5,
        )

        assertEquals(AndroidLeaseFailureAction.Wait, policy.leaseFailureAction(1))
        assertEquals(
            AndroidLeaseFailureAction.Recover,
            policy.leaseFailureAction(2, routedInternetHealthy = false),
        )
        assertEquals(
            AndroidLeaseFailureAction.Wait,
            policy.leaseFailureAction(3, routedInternetHealthy = false),
        )
        assertEquals(
            AndroidLeaseFailureAction.Stop,
            policy.leaseFailureAction(5, routedInternetHealthy = false),
        )
        assertEquals(
            AndroidLeaseFailureAction.Stop,
            policy.leaseFailureAction(8, routedInternetHealthy = false),
        )
    }

    @Test
    fun `healthy or unknown routed Internet preserves the active tunnel`() {
        val policy = AndroidRecoveryPolicy(recoverAfterFailures = 2, stopAfterFailures = 5)

        assertEquals(
            AndroidLeaseFailureAction.Wait,
            policy.leaseFailureAction(2, routedInternetHealthy = true),
        )
        assertEquals(
            AndroidLeaseFailureAction.Wait,
            policy.leaseFailureAction(5, routedInternetHealthy = true),
        )
        assertEquals(
            AndroidLeaseFailureAction.Wait,
            policy.leaseFailureAction(8, routedInternetHealthy = null),
        )
        assertEquals(false, policy.shouldVerifyRoute(1))
        assertEquals(true, policy.shouldVerifyRoute(2))
    }

    @Test
    fun `authorization loss re-enrolls while valid access remains and stops after access expiry`() {
        val policy = AndroidRecoveryPolicy()

        assertEquals(
            AndroidLeaseFailureAction.Reauthorize,
            policy.leaseFailureAction(
                consecutiveFailures = 1,
                authorizationLost = true,
                routedInternetHealthy = true,
            ),
        )
        assertEquals(
            AndroidLeaseFailureAction.Stop,
            policy.leaseFailureAction(
                consecutiveFailures = 1,
                authorizationLost = true,
                routedInternetHealthy = true,
                accessExpired = true,
            ),
        )
        assertEquals(false, policy.shouldVerifyRoute(5, authorizationLost = true))
    }

    private data class TestTransport(
        override val id: String,
        override val priority: Int,
        override val protocol: KetProtocol = KetProtocol.Reality,
    ) : AndroidTransport {
        override val endpoint: String = "vpn.example.test"
        override val port: Int = 443
        override val displayName: String = id
    }
}
