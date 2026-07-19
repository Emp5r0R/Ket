package com.ket.android

import java.util.concurrent.atomic.AtomicBoolean

/** Serializes lease refreshes and ignores expected network suspension while Android is idle. */
internal class AndroidLeaseRefreshGate {
    private val inFlight = AtomicBoolean()

    fun tryStart(stopping: Boolean, deviceIdle: Boolean): Boolean =
        !stopping && !deviceIdle && inFlight.compareAndSet(false, true)

    fun finish() {
        inFlight.set(false)
    }
}
