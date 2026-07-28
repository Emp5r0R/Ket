package com.ket.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidVpnServiceLifecycleTest {
    @Test
    fun `a second connection cannot enter a service instance that is still tearing down`() {
        val lifecycle = AndroidVpnServiceLifecycle()

        assertTrue(lifecycle.tryStart())
        assertTrue(lifecycle.tryStop())
        assertTrue(lifecycle.isStopping())
        assertFalse(lifecycle.tryStart())

        val terminal = TunnelSnapshot(phase = TunnelPhase.Disconnected)
        lifecycle.stageTerminal(terminal)

        assertEquals(terminal, lifecycle.destroy())
        assertFalse(lifecycle.tryStart())
        assertTrue(lifecycle.isStopping())
        assertTrue(AndroidVpnServiceLifecycle().tryStart())
    }

    @Test
    fun `duplicate stop and destroy calls are idempotent`() {
        val lifecycle = AndroidVpnServiceLifecycle()

        assertTrue(lifecycle.tryStop())
        assertFalse(lifecycle.tryStop())
        lifecycle.stageTerminal(TunnelSnapshot(phase = TunnelPhase.Failed, message = "failed"))

        assertEquals(TunnelPhase.Failed, lifecycle.destroy()?.phase)
        assertNull(lifecycle.destroy())
        assertFalse(lifecycle.tryStop())
        assertTrue(lifecycle.isStopping())
        lifecycle.stageTerminal(TunnelSnapshot(phase = TunnelPhase.Disconnected))
        assertNull(lifecycle.destroy())
    }
}
