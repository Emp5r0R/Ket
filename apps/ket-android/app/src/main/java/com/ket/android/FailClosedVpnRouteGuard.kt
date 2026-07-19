package com.ket.android

/** Keeps the latest VPN route installed until a replacement exists or shutdown is explicit. */
internal class FailClosedVpnRouteGuard<T : AutoCloseable> : AutoCloseable {
    private var active: T? = null

    @Synchronized
    fun replace(replacement: T) {
        val previous = active
        active = replacement
        if (previous !== replacement) runCatching { previous?.close() }
    }

    @Synchronized
    override fun close() {
        val previous = active
        active = null
        runCatching { previous?.close() }
    }
}
