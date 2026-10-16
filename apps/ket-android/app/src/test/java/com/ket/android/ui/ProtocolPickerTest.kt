package com.ket.android.ui

import com.ket.android.KetProtocol
import com.ket.android.TunnelPhase
import com.ket.android.TunnelSnapshot
import com.ket.android.ui.connection.islandDetail
import com.ket.android.ui.connection.pickerChoices
import com.ket.android.ui.connection.serverHost
import org.junit.Assert.assertEquals
import org.junit.Test

class ProtocolPickerTest {
    @Test
    fun `only offered protocols are listed, plus a saved choice no longer offered`() {
        val offered = listOf(KetProtocol.Stealth, KetProtocol.Reality)
        assertEquals(offered, pickerChoices(offered, null))
        assertEquals(offered, pickerChoices(offered, KetProtocol.Stealth))
        assertEquals(offered + KetProtocol.Hysteria2, pickerChoices(offered, KetProtocol.Hysteria2))
    }

    @Test
    fun `without a session every protocol is listed`() {
        assertEquals(KetProtocol.entries, pickerChoices(emptyList(), null))
    }

    @Test
    fun `the status island explains each state in one line`() {
        assertEquals(
            "Ket is off. Your network can see what you do.",
            islandDetail(TunnelSnapshot(phase = TunnelPhase.Disconnected)),
        )
        assertEquals(
            "Protected with HTTPS Stealth",
            islandDetail(TunnelSnapshot(phase = TunnelPhase.Connected, transportName = "HTTPS Stealth")),
        )
        assertEquals(
            "Starting VLESS + REALITY...",
            islandDetail(TunnelSnapshot(phase = TunnelPhase.Connecting, message = "Starting VLESS + REALITY...")),
        )
        assertEquals(
            "Could not connect. Open diagnostics for details.",
            islandDetail(TunnelSnapshot(phase = TunnelPhase.Failed)),
        )
    }

    @Test
    fun `the server is shown by its host`() {
        assertEquals("ket.example.com", serverHost("https://ket.example.com/"))
        assertEquals("Saved server", serverHost(""))
    }
}
