package com.ket.android

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class KetTunnelRuntimeTest {
    @After
    fun resetRuntime() {
        KetTunnelRuntime.clearPending()
        KetTunnelRuntime.publish(TunnelSnapshot())
    }

    @Test
    fun `subscription immediately observes current state and close removes listener`() {
        val observed = mutableListOf<TunnelSnapshot>()
        val subscription = KetTunnelRuntime.subscribe(observed::add)
        assertEquals(listOf(TunnelSnapshot()), observed)

        KetTunnelRuntime.publish(TunnelSnapshot(phase = TunnelPhase.Connecting, message = "Starting"))
        KetTunnelRuntime.update { it.copy(phase = TunnelPhase.Connected, sentBytes = 100) }
        assertEquals(
            listOf(TunnelPhase.Disconnected, TunnelPhase.Connecting, TunnelPhase.Connected),
            observed.map(TunnelSnapshot::phase),
        )
        assertEquals(100, KetTunnelRuntime.snapshot().sentBytes)

        subscription.close()
        KetTunnelRuntime.publish(TunnelSnapshot(phase = TunnelPhase.Stopping))
        assertEquals(3, observed.size)
    }

    @Test
    fun `pending launch has single owner and take is destructive`() {
        val first = launchSpec("secret-session-token")
        KetTunnelRuntime.offer(first)
        assertThrows(IllegalStateException::class.java) {
            KetTunnelRuntime.offer(launchSpec("other-session-token"))
        }
        assertSame(first, KetTunnelRuntime.take())
        assertNull(KetTunnelRuntime.take())

        val second = launchSpec("second-session-token")
        KetTunnelRuntime.offer(second)
        assertSame(second, KetTunnelRuntime.clearPending())
        assertNull(KetTunnelRuntime.clearPending())
    }

    @Test
    fun `launch specs preserve enrollment choices without exposing session tokens`() {
        val token = "A".repeat(44)
        val result = EnrollmentResult(
            token = token,
            expiresAtEpochSeconds = 2_000,
            accessExpiresAtEpochSeconds = 3_000,
            node = node(),
            transports = emptyList(),
            manifestJson = "{}",
        )
        val spec = TunnelLaunchSpec.fromEnrollment(
            "https://ket.example.test",
            result,
            KetProtocol.Stealth,
        )

        assertEquals(token, spec.sessionToken)
        assertEquals(3_000L, spec.accessExpiresAtEpochSeconds)
        assertEquals(KetProtocol.Stealth, spec.preferredProtocol)
        assertTrue(spec.toString().contains("[REDACTED]"))
        assertFalse(spec.toString().contains(token))
    }

    private fun launchSpec(token: String): TunnelLaunchSpec = TunnelLaunchSpec(
        controlEndpoint = "https://ket.example.test",
        sessionToken = token,
        sessionExpiresAtEpochSeconds = 4_000_000_000,
        node = node(),
        transports = emptyList(),
        accessExpiresAtEpochSeconds = 3_000,
        preferredProtocol = null,
    )

    private fun node(): AndroidNodeStatus = AndroidNodeStatus(
        id = "sg-01",
        displayName = "Ket Singapore",
        publicUrl = "https://ket.example.test",
        location = AndroidNodeLocation("SG", "Singapore", "Singapore", 1.29, 103.85),
        health = AndroidNodeHealth.Healthy,
        activeSessions = 1,
        sessionCapacity = 10,
        capacityPercent = 10.0,
        cpuLoadPercent = 5.0,
        memoryUsedBytes = 1_024,
        memoryTotalBytes = 4_096,
        uptimeSeconds = 60,
        observedAtEpochSeconds = 1_000,
    )
}
