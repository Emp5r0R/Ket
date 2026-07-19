package com.ket.android

internal class AndroidTransportHistory(
    private val baseCooldownMillis: Long = 15_000,
    private val maximumCooldownMillis: Long = 120_000,
) {
    private data class Record(
        val consecutiveFailures: Int,
        val lastLatencyMillis: Long?,
        val cooldownUntilMillis: Long,
    )

    private val records = mutableMapOf<String, Record>()

    init {
        require(baseCooldownMillis > 0) { "Transport cooldown must be positive" }
        require(maximumCooldownMillis >= baseCooldownMillis) {
            "Maximum transport cooldown must not be shorter than its base"
        }
    }

    fun rank(transports: List<AndroidTransport>, nowMillis: Long): List<AndroidTransport> =
        transports.sortedWith(
            compareBy<AndroidTransport>(
                { if (records[it.id]?.cooldownUntilMillis?.let { until -> until > nowMillis } == true) 1 else 0 },
                { records[it.id]?.consecutiveFailures ?: 0 },
                AndroidTransport::priority,
                { records[it.id]?.lastLatencyMillis ?: DEFAULT_LATENCY_MILLIS },
                AndroidTransport::id,
            ),
        )

    fun recordSuccess(id: String, latencyMillis: Long) {
        records[id] = Record(
            consecutiveFailures = 0,
            lastLatencyMillis = latencyMillis.coerceAtLeast(0),
            cooldownUntilMillis = 0,
        )
    }

    fun recordFailure(id: String, nowMillis: Long) {
        val previous = records[id]
        val failures = (previous?.consecutiveFailures ?: 0).let {
            if (it == Int.MAX_VALUE) it else it + 1
        }
        val multiplier = 1L shl (failures - 1).coerceAtMost(3)
        val duration = baseCooldownMillis
            .coerceAtMost(maximumCooldownMillis / multiplier)
            .times(multiplier)
            .coerceAtMost(maximumCooldownMillis)
        records[id] = Record(
            consecutiveFailures = failures,
            lastLatencyMillis = previous?.lastLatencyMillis,
            cooldownUntilMillis = if (nowMillis > Long.MAX_VALUE - duration) {
                Long.MAX_VALUE
            } else {
                nowMillis + duration
            },
        )
    }

    private companion object {
        const val DEFAULT_LATENCY_MILLIS = 500L
    }
}

internal enum class AndroidLeaseFailureAction {
    Wait,
    Recover,
    Stop,
}

internal class AndroidRecoveryPolicy(
    val maximumReconnectRounds: Int = 3,
    private val recoverAfterFailures: Int = 2,
    private val stopAfterFailures: Int = 5,
) {
    init {
        require(maximumReconnectRounds > 0) { "Recovery must allow at least one reconnect round" }
        require(recoverAfterFailures > 0) { "Recovery threshold must be positive" }
        require(stopAfterFailures > recoverAfterFailures) {
            "Stop threshold must leave room for recovery"
        }
    }

    fun leaseFailureAction(
        consecutiveFailures: Int,
        authorizationLost: Boolean = false,
    ): AndroidLeaseFailureAction = when {
        authorizationLost -> AndroidLeaseFailureAction.Stop
        consecutiveFailures >= stopAfterFailures -> AndroidLeaseFailureAction.Stop
        consecutiveFailures == recoverAfterFailures -> AndroidLeaseFailureAction.Recover
        else -> AndroidLeaseFailureAction.Wait
    }
}
