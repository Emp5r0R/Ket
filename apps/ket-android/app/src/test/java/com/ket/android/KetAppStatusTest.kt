package com.ket.android

import org.junit.Assert.assertEquals
import org.junit.Test

class KetAppStatusTest {
    @Test
    fun `connection copy names the watched and liberated states`() {
        assertEquals("You are being watched", phaseStatusLabel(TunnelPhase.Disconnected))
        assertEquals("Liberated", phaseStatusLabel(TunnelPhase.Connected))
        assertEquals("Connect to get liberated", phaseHeadline(TunnelPhase.Disconnected))
        assertEquals("Liberated route", phaseHeadline(TunnelPhase.Connected))
    }

    @Test
    fun `transitional status copy remains explicit`() {
        assertEquals("Authorizing", phaseStatusLabel(TunnelPhase.Enrolling))
        assertEquals("Connecting", phaseStatusLabel(TunnelPhase.Connecting))
        assertEquals("Recovering", phaseStatusLabel(TunnelPhase.Reconnecting))
        assertEquals("Stopping", phaseStatusLabel(TunnelPhase.Stopping))
        assertEquals("Attention", phaseStatusLabel(TunnelPhase.Failed))
    }

    @Test
    fun `surveillance status alternates deterministically without changing connection state`() {
        assertEquals("You are being watched", disconnectedSurveillanceLabel(0))
        assertEquals("You are under surveillance", disconnectedSurveillanceLabel(1))
        assertEquals("You are being watched", disconnectedSurveillanceLabel(2))
        assertEquals("You are under surveillance", disconnectedSurveillanceLabel(-1))
    }

    @Test
    fun `connection actions remain explicit and cancellable in every phase`() {
        assertEquals("Connect tunnel", connectionActionLabel(TunnelPhase.Disconnected))
        assertEquals("Cancel connection attempt", connectionActionLabel(TunnelPhase.Enrolling))
        assertEquals("Cancel connection attempt", connectionActionLabel(TunnelPhase.Connecting))
        assertEquals("Cancel connection attempt", connectionActionLabel(TunnelPhase.Reconnecting))
        assertEquals("Disconnect tunnel", connectionActionLabel(TunnelPhase.Connected))
        assertEquals("Disconnecting tunnel", connectionActionLabel(TunnelPhase.Stopping))
        assertEquals("Connect tunnel", connectionActionLabel(TunnelPhase.Failed))
    }

    @Test
    fun `power copy matches desktop route language`() {
        assertEquals("Connect to get liberated", connectionPowerHeadline(TunnelPhase.Disconnected))
        assertEquals("You are under surveillance", connectionPowerDetail(TunnelPhase.Disconnected))
        assertEquals("Liberated", connectionPowerHeadline(TunnelPhase.Connected))
        assertEquals("Protected route active", connectionPowerDetail(TunnelPhase.Connected))
        assertEquals("Connection failed", connectionPowerHeadline(TunnelPhase.Failed))
        assertEquals("Open diagnostics, then try again", connectionPowerDetail(TunnelPhase.Failed))
    }

    @Test
    fun `route heading uses verified node location without duplicate country text`() {
        val singapore = node(AndroidNodeLocation("SG", "Singapore", "Singapore", 1.29, 103.85))
        val countryOnly = node(AndroidNodeLocation("SG", "Singapore", null, 1.29, 103.85))

        assertEquals("Singapore, Singapore", connectionLocationHeading(singapore))
        assertEquals("Singapore", connectionLocationHeading(countryOnly))
        assertEquals("Choose your node", connectionLocationHeading(null))
    }

    private fun node(location: AndroidNodeLocation) = AndroidNodeStatus(
        id = "sg-01",
        displayName = "Ket Singapore",
        publicUrl = "https://ket.example.test",
        location = location,
        health = AndroidNodeHealth.Healthy,
        activeSessions = 1,
        sessionCapacity = 4,
        capacityPercent = 25.0,
        cpuLoadPercent = 5.0,
        memoryUsedBytes = 1_024,
        memoryTotalBytes = 4_096,
        uptimeSeconds = 60,
        observedAtEpochSeconds = 1_000,
    )
}
