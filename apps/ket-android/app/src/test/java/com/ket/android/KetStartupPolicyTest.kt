package com.ket.android

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class KetStartupPolicyTest {
    @Test
    fun `new installs render immediately and never request an automatic connection`() {
        TunnelPhase.entries.forEach { phase ->
            val decision = KetStartupPolicy.initial(hasSavedProfile = false, phase)
            assertTrue(decision.mode == KetStartupMode.Ready)
            assertFalse(decision.requestSavedProfileConnection)
            assertFalse(decision.observeExistingConnection)
        }
    }

    @Test
    fun `saved disconnected and failed profiles restore without exposing enrollment`() {
        listOf(TunnelPhase.Disconnected, TunnelPhase.Failed).forEach { phase ->
            val decision = KetStartupPolicy.initial(hasSavedProfile = true, phase)
            assertTrue(decision.mode == KetStartupMode.RestoringProfile)
            assertTrue(decision.requestSavedProfileConnection)
            assertFalse(decision.observeExistingConnection)
        }
    }

    @Test
    fun `active saved connection attempts are observed without duplicate enrollment`() {
        listOf(TunnelPhase.Enrolling, TunnelPhase.Connecting, TunnelPhase.Reconnecting).forEach { phase ->
            val decision = KetStartupPolicy.initial(hasSavedProfile = true, phase)
            assertTrue(decision.mode == KetStartupMode.RestoringProfile)
            assertFalse(decision.requestSavedProfileConnection)
            assertTrue(decision.observeExistingConnection)
        }
    }

    @Test
    fun `connected and stopping states never trigger an unsafe duplicate start`() {
        listOf(TunnelPhase.Connected, TunnelPhase.Stopping).forEach { phase ->
            val decision = KetStartupPolicy.initial(hasSavedProfile = true, phase)
            assertTrue(decision.mode == KetStartupMode.Ready)
            assertFalse(decision.requestSavedProfileConnection)
            assertFalse(decision.observeExistingConnection)
        }
    }

    @Test
    fun `restore screen settles only after a started attempt reaches a terminal result`() {
        TunnelPhase.entries.forEach { phase ->
            assertFalse(KetStartupPolicy.restoreFinished(restoreStarted = false, phase))
        }
        listOf(TunnelPhase.Disconnected, TunnelPhase.Enrolling, TunnelPhase.Connecting, TunnelPhase.Reconnecting).forEach {
            assertFalse(KetStartupPolicy.restoreFinished(restoreStarted = true, it))
        }
        assertTrue(KetStartupPolicy.restoreFinished(restoreStarted = true, TunnelPhase.Connected))
        assertTrue(KetStartupPolicy.restoreFinished(restoreStarted = true, TunnelPhase.Failed))
    }

    @Test
    fun `startup presentation is fixed and cannot echo durable credentials`() {
        val presentation = KetStartupPolicy.presentation()
        val rendered = "${presentation.title}\n${presentation.detail}"

        assertTrue(rendered.contains("Restoring saved server"))
        assertFalse(rendered.contains("https://private.example"))
        assertFalse(rendered.contains("S".repeat(32)))
    }
}
