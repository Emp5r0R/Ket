package com.ket.android

import org.junit.Assert.assertThrows
import org.junit.Test

class AndroidTransportEngineTest {
    @Test
    fun cancelledStartupStopsAtCooperativeCheck() {
        assertThrows(InterruptedException::class.java) {
            ensureEngineStartActive { true }
        }
    }

    @Test
    fun activeStartupContinuesPastCooperativeCheck() {
        ensureEngineStartActive { false }
    }
}
