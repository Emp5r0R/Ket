package com.ket.android

import java.util.concurrent.atomic.AtomicReference

/** Keeps a service instance single-use so a replacement cannot race its asynchronous teardown. */
internal class AndroidVpnServiceLifecycle {
    private enum class State {
        Idle,
        Active,
        Stopping,
        Destroyed,
    }

    private val state = AtomicReference(State.Idle)
    private val terminalSnapshot = AtomicReference<TunnelSnapshot?>()

    fun tryStart(): Boolean = state.compareAndSet(State.Idle, State.Active)

    fun tryStop(): Boolean {
        while (true) {
            when (val current = state.get()) {
                State.Stopping, State.Destroyed -> return false
                State.Idle, State.Active -> if (state.compareAndSet(current, State.Stopping)) return true
            }
        }
    }

    fun isStopping(): Boolean {
        val current = state.get()
        return current == State.Stopping || current == State.Destroyed
    }

    @Synchronized
    fun stageTerminal(snapshot: TunnelSnapshot) {
        if (state.get() == State.Destroyed) return
        check(state.get() == State.Stopping) { "A terminal VPN state requires an active stop" }
        check(terminalSnapshot.compareAndSet(null, snapshot)) { "A terminal VPN state is already staged" }
    }

    /** The terminal UI state is deliberately withheld until this service can no longer receive starts. */
    @Synchronized
    fun destroy(): TunnelSnapshot? {
        state.set(State.Destroyed)
        return terminalSnapshot.getAndSet(null)
    }
}
