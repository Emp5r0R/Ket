package com.ket.android

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidLeaseRefreshGateTest {
    @Test
    fun `Doze and shutdown suppress lease refresh`() {
        val gate = AndroidLeaseRefreshGate()

        assertFalse(gate.tryStart(stopping = false, deviceIdle = true))
        assertFalse(gate.tryStart(stopping = true, deviceIdle = false))
    }

    @Test
    fun `only one refresh runs until completion`() {
        val gate = AndroidLeaseRefreshGate()

        assertTrue(gate.tryStart(stopping = false, deviceIdle = false))
        assertFalse(gate.tryStart(stopping = false, deviceIdle = false))
        gate.finish()
        assertTrue(gate.tryStart(stopping = false, deviceIdle = false))
    }
}
