package com.ket.android

/**
 * Decides whether a move to another network (Wi-Fi to mobile data, a new hotspot) broke the
 * protected route. Most engines open new connections over the new network on their own, so the
 * route usually keeps working; restarting the engine then would only interrupt it. The route is
 * checked after the engines have had time to redial, and once more before giving up.
 */
internal class NetworkChangeVerifier(
    /** True when traffic reaches the Internet through the tunnel. */
    private val routeWorks: () -> Boolean,
    private val pause: (Long) -> Unit = Thread::sleep,
    private val settleMillis: Long = 3_000,
    private val retryMillis: Long = 2_000,
    private val attempts: Int = 2,
) {
    init {
        require(attempts > 0) { "Verification needs at least one attempt" }
    }

    /** True when the route still works; false when the tunnel must be replaced or stop was asked. */
    fun routeSurvived(cancelled: () -> Boolean): Boolean {
        pause(settleMillis)
        repeat(attempts) { attempt ->
            if (cancelled()) return false
            if (runCatching(routeWorks).getOrDefault(false)) return true
            if (attempt + 1 < attempts) pause(retryMillis)
        }
        return false
    }
}
