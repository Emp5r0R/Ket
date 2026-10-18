package com.ket.android

import org.json.JSONObject

internal class AndroidTransportHistory(
    private val baseCooldownMillis: Long = 15_000,
    private val maximumCooldownMillis: Long = 120_000,
) {
    private data class Record(
        val consecutiveFailures: Int,
        val lastLatencyMillis: Long?,
        val cooldownUntilMillis: Long,
        /** Set by the last success: on this network it is the last known good. */
        val worked: Boolean = false,
    )

    private val records = mutableMapOf<String, Record>()

    init {
        require(baseCooldownMillis > 0) { "Transport cooldown must be positive" }
        require(maximumCooldownMillis >= baseCooldownMillis) {
            "Maximum transport cooldown must not be shorter than its base"
        }
    }

    /**
     * An explicit [preferredProtocol] filters to that protocol. Otherwise (Automatic) the
     * [automaticExcluded] protocols are left out, unless that would leave nothing.
     */
    @Synchronized
    fun rank(
        transports: List<AndroidTransport>,
        nowMillis: Long,
        preferredProtocol: KetProtocol? = null,
        automaticExcluded: Set<KetProtocol> = emptySet(),
    ): List<AndroidTransport> =
        transports
            .filter { preferredProtocol == null || it.protocol == preferredProtocol }
            .let { candidates ->
                val allowed = candidates.filter { it.protocol !in automaticExcluded }
                if (preferredProtocol == null && allowed.isNotEmpty()) allowed else candidates
            }
            .sortedWith(
                compareBy<AndroidTransport>(
                    { if (records[it.id]?.cooldownUntilMillis?.let { until -> until > nowMillis } == true) 1 else 0 },
                    { records[it.id]?.consecutiveFailures ?: 0 },
                    { if (records[it.id]?.worked == true) 0 else 1 },
                    AndroidTransport::priority,
                    { records[it.id]?.lastLatencyMillis ?: DEFAULT_LATENCY_MILLIS },
                    AndroidTransport::id,
                ),
            )

    @Synchronized
    fun recordSuccess(id: String, latencyMillis: Long) {
        records[id] = Record(
            consecutiveFailures = 0,
            lastLatencyMillis = latencyMillis.coerceAtLeast(0),
            cooldownUntilMillis = 0,
            worked = true,
        )
    }

    /**
     * The outcomes worth keeping for this network. Cooldowns are left out: they count on the
     * device's uptime clock, which restarts with the phone.
     */
    @Synchronized
    fun export(): JSONObject = JSONObject().apply {
        records.entries.take(MAX_RECORDS).forEach { (id, record) ->
            put(
                id,
                JSONObject()
                    .put("failures", record.consecutiveFailures)
                    .put("worked", record.worked)
                    .apply { record.lastLatencyMillis?.let { put("latency", it) } },
            )
        }
    }

    /** Replaces the history with one saved for the network the phone is on now. */
    @Synchronized
    fun restore(saved: JSONObject?) {
        records.clear()
        saved ?: return
        saved.keys().asSequence().take(MAX_RECORDS).forEach { id ->
            val record = saved.optJSONObject(id) ?: return@forEach
            records[id] = Record(
                consecutiveFailures = record.optInt("failures", 0).coerceIn(0, 1_000),
                lastLatencyMillis = record.optLong("latency", -1).takeIf { it >= 0 },
                cooldownUntilMillis = 0,
                worked = record.optBoolean("worked", false),
            )
        }
    }

    @Synchronized
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
            worked = false,
            cooldownUntilMillis = if (nowMillis > Long.MAX_VALUE - duration) {
                Long.MAX_VALUE
            } else {
                nowMillis + duration
            },
        )
    }

    private companion object {
        const val DEFAULT_LATENCY_MILLIS = 500L
        const val MAX_RECORDS = 32
    }
}

internal enum class AndroidLeaseFailureAction {
    Wait,
    Recover,
    Reauthorize,
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
        routedInternetHealthy: Boolean? = null,
        accessExpired: Boolean = false,
    ): AndroidLeaseFailureAction = when {
        authorizationLost && !accessExpired -> AndroidLeaseFailureAction.Reauthorize
        authorizationLost -> AndroidLeaseFailureAction.Stop
        routedInternetHealthy != false -> AndroidLeaseFailureAction.Wait
        consecutiveFailures >= stopAfterFailures -> AndroidLeaseFailureAction.Stop
        consecutiveFailures == recoverAfterFailures -> AndroidLeaseFailureAction.Recover
        else -> AndroidLeaseFailureAction.Wait
    }

    fun shouldVerifyRoute(
        consecutiveFailures: Int,
        authorizationLost: Boolean = false,
    ): Boolean = !authorizationLost && consecutiveFailures >= recoverAfterFailures
}
