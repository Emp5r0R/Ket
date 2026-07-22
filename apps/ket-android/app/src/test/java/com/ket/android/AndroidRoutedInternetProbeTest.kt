package com.ket.android

import java.util.concurrent.atomic.AtomicInteger
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidRoutedInternetProbeTest {
    @Test
    fun `accepts first endpoint carrying routed traffic`() {
        val attempts = AtomicInteger()
        val probe = RoutedInternetProbe(listOf("first", "second")) {
            attempts.incrementAndGet()
            it == "first"
        }

        probe.verify()

        assertEquals(1, attempts.get())
    }

    @Test
    fun `falls back across blocked connectivity endpoints`() {
        val attempts = mutableListOf<String>()
        val probe = RoutedInternetProbe(listOf("blocked", "working")) {
            attempts += it
            it == "working"
        }

        probe.verify()

        assertEquals(listOf("blocked", "working"), attempts)
    }

    @Test
    fun `rejects listener-only connections with no Internet path`() {
        val error = assertThrows(IllegalStateException::class.java) {
            RoutedInternetProbe(listOf("first", "second")) { false }.verify()
        }

        assertTrue(error.message.orEmpty().contains("carried no Internet traffic"))
    }

    @Test
    fun `honors disconnect before probing`() {
        val error = assertThrows(InterruptedException::class.java) {
            RoutedInternetProbe(listOf("first")) { true }.verify { true }
        }

        assertTrue(error.message.orEmpty().contains("cancelled"))
    }
}
