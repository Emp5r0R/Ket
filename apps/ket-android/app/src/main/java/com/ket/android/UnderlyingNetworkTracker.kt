package com.ket.android

/** Tracks physical-network set changes without coupling recovery policy to Android callbacks. */
internal class UnderlyingNetworkTracker<T> {
    private val available = linkedSetOf<T>()
    private var connectedBaseline: Set<T>? = null
    private var awaitingInitialBaseline = false
    private var sawOutage = false

    @Synchronized
    fun onAvailable(network: T): Boolean = available.add(network)

    @Synchronized
    fun onLost(network: T): Boolean {
        val changed = available.remove(network)
        if (changed && connectedBaseline != null && available.isEmpty()) {
            awaitingInitialBaseline = false
            sawOutage = true
        }
        return changed
    }

    @Synchronized
    fun markConnected() {
        connectedBaseline = available.toSet()
        awaitingInitialBaseline = available.isEmpty()
        sawOutage = false
    }

    @Synchronized
    fun clearConnection() {
        connectedBaseline = null
        awaitingInitialBaseline = false
        sawOutage = false
    }

    @Synchronized
    fun isConnected(): Boolean = connectedBaseline != null

    @Synchronized
    fun consumeRecoveryRequired(): Boolean {
        val baseline = connectedBaseline ?: return false
        if (available.isEmpty()) {
            sawOutage = true
            return false
        }
        val current = available.toSet()
        if (awaitingInitialBaseline) {
            connectedBaseline = current
            awaitingInitialBaseline = false
            sawOutage = false
            return false
        }
        val changed = sawOutage || current != baseline
        if (changed) {
            connectedBaseline = current
            sawOutage = false
        }
        return changed
    }
}
