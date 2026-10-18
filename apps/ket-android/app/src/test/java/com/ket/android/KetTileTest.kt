package com.ket.android

import org.junit.Assert.assertEquals
import org.junit.Test

class KetTileTest {
    @Test
    fun `the tile disconnects while Ket is on or starting, and waits while it stops`() {
        listOf(TunnelPhase.Connected, TunnelPhase.Enrolling, TunnelPhase.Connecting, TunnelPhase.Reconnecting).forEach {
            assertEquals(it.name, TileAction.Disconnect, tileAction(it, vpnPermitted = true, profileSaved = true))
        }
        assertEquals(TileAction.Wait, tileAction(TunnelPhase.Stopping, vpnPermitted = true, profileSaved = true))
    }

    @Test
    fun `the tile connects the saved server only with VPN permission, and opens the app otherwise`() {
        listOf(TunnelPhase.Disconnected, TunnelPhase.Failed).forEach {
            assertEquals(TileAction.Connect, tileAction(it, vpnPermitted = true, profileSaved = true))
            assertEquals(TileAction.OpenApp, tileAction(it, vpnPermitted = false, profileSaved = true))
            assertEquals(TileAction.OpenApp, tileAction(it, vpnPermitted = true, profileSaved = false))
        }
    }

    @Test
    fun `the subtitle names the protocol while connected`() {
        assertEquals("Hysteria XOR", tileSubtitle(TunnelSnapshot(phase = TunnelPhase.Connected, transportName = "Hysteria XOR")))
        assertEquals("Connecting", tileSubtitle(TunnelSnapshot(phase = TunnelPhase.Connecting)))
        assertEquals("Off", tileSubtitle(TunnelSnapshot()))
    }
}
