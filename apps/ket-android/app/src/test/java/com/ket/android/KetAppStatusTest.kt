package com.ket.android

import org.junit.Assert.assertEquals
import org.junit.Test

class KetAppStatusTest {
    @Test
    fun `connection copy names the watched and liberated states`() {
        assertEquals("You are being watched", phaseStatusLabel(TunnelPhase.Disconnected))
        assertEquals("Liberated", phaseStatusLabel(TunnelPhase.Connected))
        assertEquals("Choose your node", phaseHeadline(TunnelPhase.Disconnected))
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
}
