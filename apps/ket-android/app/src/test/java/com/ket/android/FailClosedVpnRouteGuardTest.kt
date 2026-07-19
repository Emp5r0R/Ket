package com.ket.android

import org.junit.Assert.assertEquals
import org.junit.Test

class FailClosedVpnRouteGuardTest {
    @Test
    fun `first route remains open as the fail-closed guard`() {
        val route = TestRoute()
        val guard = FailClosedVpnRouteGuard<TestRoute>()

        guard.replace(route)

        assertEquals(0, route.closeCount)
    }

    @Test
    fun `replacement closes the old route only after the new route is installed`() {
        val old = TestRoute()
        val replacement = TestRoute()
        val guard = FailClosedVpnRouteGuard<TestRoute>()
        guard.replace(old)

        guard.replace(replacement)

        assertEquals(1, old.closeCount)
        assertEquals(0, replacement.closeCount)
    }

    @Test
    fun `final close removes the active route exactly once`() {
        val route = TestRoute()
        val guard = FailClosedVpnRouteGuard<TestRoute>()
        guard.replace(route)

        guard.close()
        guard.close()

        assertEquals(1, route.closeCount)
    }

    private class TestRoute : AutoCloseable {
        var closeCount = 0

        override fun close() {
            closeCount += 1
        }
    }
}
