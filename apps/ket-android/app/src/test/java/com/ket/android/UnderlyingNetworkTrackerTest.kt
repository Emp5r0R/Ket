package com.ket.android

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class UnderlyingNetworkTrackerTest {
    @Test
    fun callbacksBeforeConnectionBecomeTheBaseline() {
        val tracker = UnderlyingNetworkTracker<String>()

        assertTrue(tracker.onAvailable("wifi"))
        assertFalse(tracker.isConnected())
        tracker.markConnected()

        assertFalse(tracker.consumeRecoveryRequired())
        assertFalse(tracker.onAvailable("wifi"))
        assertFalse(tracker.consumeRecoveryRequired())
    }

    @Test
    fun aNewNetworkRequiresOneRecovery() {
        val tracker = connectedTracker("cellular")

        assertTrue(tracker.onAvailable("wifi"))
        assertTrue(tracker.consumeRecoveryRequired())
        assertFalse(tracker.consumeRecoveryRequired())
    }

    @Test
    fun lateInitialCallbackSeedsTheConnectedBaseline() {
        val tracker = UnderlyingNetworkTracker<String>()
        tracker.markConnected()

        assertTrue(tracker.onAvailable("wifi"))
        assertFalse(tracker.consumeRecoveryRequired())
        assertTrue(tracker.onAvailable("cellular"))
        assertTrue(tracker.consumeRecoveryRequired())
    }

    @Test
    fun switchCallbacksCollapseIntoOneCurrentSet() {
        val tracker = connectedTracker("cellular")

        assertTrue(tracker.onAvailable("wifi"))
        assertTrue(tracker.onLost("cellular"))
        assertTrue(tracker.consumeRecoveryRequired())
        assertFalse(tracker.consumeRecoveryRequired())
    }

    @Test
    fun completeOutageWaitsUntilConnectivityReturns() {
        val tracker = connectedTracker("wifi")

        assertTrue(tracker.onLost("wifi"))
        assertFalse(tracker.consumeRecoveryRequired())
        assertTrue(tracker.onAvailable("wifi"))
        assertTrue(tracker.consumeRecoveryRequired())
    }

    @Test
    fun lateInitialNetworkThatFlapsStillRecoversOnReturn() {
        val tracker = UnderlyingNetworkTracker<String>()
        tracker.markConnected()
        tracker.onAvailable("wifi")
        tracker.onLost("wifi")

        assertFalse(tracker.consumeRecoveryRequired())
        tracker.onAvailable("wifi")
        assertTrue(tracker.consumeRecoveryRequired())
    }

    @Test
    fun disconnectClearsPendingNetworkChanges() {
        val tracker = connectedTracker("wifi")
        tracker.onAvailable("cellular")

        tracker.clearConnection()

        assertFalse(tracker.isConnected())
        assertFalse(tracker.consumeRecoveryRequired())
    }

    private fun connectedTracker(network: String): UnderlyingNetworkTracker<String> =
        UnderlyingNetworkTracker<String>().also {
            it.onAvailable(network)
            it.markConnected()
        }
}
