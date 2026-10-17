package com.ket.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class NetworkChangeVerifierTest {
    private val pauses = mutableListOf<Long>()

    private fun verifier(vararg results: Boolean): Pair<NetworkChangeVerifier, () -> Int> {
        var calls = 0
        val verifier = NetworkChangeVerifier(
            routeWorks = { results[calls++] },
            pause = { pauses += it },
        )
        return verifier to { calls }
    }

    @Test
    fun `a route that kept working needs nothing after the engines settle`() {
        val (verifier, calls) = verifier(true)
        assertTrue(verifier.routeSurvived { false })
        assertEquals(1, calls())
        assertEquals(listOf(3_000L), pauses)
    }

    @Test
    fun `a route still redialling gets a second chance`() {
        val (verifier, calls) = verifier(false, true)
        assertTrue(verifier.routeSurvived { false })
        assertEquals(2, calls())
        assertEquals(listOf(3_000L, 2_000L), pauses)
    }

    @Test
    fun `a broken route is reported after every attempt failed or threw`() {
        var calls = 0
        val verifier = NetworkChangeVerifier(
            routeWorks = { calls++; if (calls == 1) error("probe failed") else false },
            pause = { pauses += it },
        )
        assertFalse(verifier.routeSurvived { false })
        assertEquals(2, calls)
    }

    @Test
    fun `a stop request ends the check without probing`() {
        val (verifier, calls) = verifier(true)
        assertFalse(verifier.routeSurvived { true })
        assertEquals(0, calls())
    }
}
