package com.ket.android

import android.content.Context
import android.content.Intent
import androidx.core.content.ContextCompat
import java.util.concurrent.CopyOnWriteArraySet
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

enum class TunnelPhase {
    Disconnected,
    Enrolling,
    Connecting,
    Reconnecting,
    Connected,
    Stopping,
    Failed,
}

data class TunnelSnapshot(
    val phase: TunnelPhase = TunnelPhase.Disconnected,
    val node: AndroidNodeStatus? = null,
    val message: String = "",
    val sentBytes: Long = 0,
    val receivedBytes: Long = 0,
    val onlineConnections: Int = 0,
    val handshakeLatencyMs: Long? = null,
    val transportName: String = "Auto",
    val reconnectAttempt: Int = 0,
    val accessExpiresAtEpochSeconds: Long? = null,
)

internal class TunnelLaunchSpec(
    val controlEndpoint: String,
    val sessionToken: String,
    val node: AndroidNodeStatus,
    val transports: List<AndroidTransport>,
    val accessExpiresAtEpochSeconds: Long?,
    val preferredProtocol: KetProtocol? = null,
) {
    override fun toString(): String =
        "TunnelLaunchSpec(controlEndpoint=$controlEndpoint, sessionToken=[REDACTED], node=${node.displayName}, transports=$transports)"

    companion object {
        fun fromEnrollment(
            controlEndpoint: String,
            result: EnrollmentResult,
            preferredProtocol: KetProtocol? = null,
        ): TunnelLaunchSpec =
            TunnelLaunchSpec(
                controlEndpoint,
                result.token,
                result.node,
                result.transports,
                result.accessExpiresAtEpochSeconds,
                preferredProtocol,
            )
    }
}

object KetTunnelRuntime {
    private val current = AtomicReference(TunnelSnapshot())
    private val pending = AtomicReference<TunnelLaunchSpec?>()
    private val listeners = CopyOnWriteArraySet<(TunnelSnapshot) -> Unit>()

    fun snapshot(): TunnelSnapshot = current.get()

    fun subscribe(listener: (TunnelSnapshot) -> Unit): AutoCloseable {
        listeners += listener
        listener(current.get())
        return AutoCloseable { listeners -= listener }
    }

    internal fun publish(snapshot: TunnelSnapshot) {
        current.set(snapshot)
        listeners.forEach { it(snapshot) }
    }

    internal fun update(transform: (TunnelSnapshot) -> TunnelSnapshot) {
        publish(current.updateAndGet(transform))
    }

    internal fun offer(spec: TunnelLaunchSpec) {
        check(pending.compareAndSet(null, spec)) { "A tunnel launch is already pending" }
    }

    internal fun take(): TunnelLaunchSpec? = pending.getAndSet(null)

    internal fun clearPending(): TunnelLaunchSpec? = pending.getAndSet(null)
}

object KetTunnelController {
    private val executor = Executors.newSingleThreadExecutor()
    private val generation = AtomicLong()
    private val lifecycleLock = Any()

    fun connect(
        context: Context,
        serverUrl: String,
        accessCode: String,
        preferredProtocol: KetProtocol? = null,
    ) {
        val attempt = synchronized(lifecycleLock) {
            generation.incrementAndGet().also {
                KetTunnelRuntime.publish(
                    TunnelSnapshot(phase = TunnelPhase.Enrolling, message = "Authorizing device..."),
                )
            }
        }
        val appContext = context.applicationContext
        executor.execute {
            var spec: TunnelLaunchSpec? = null
            var sessionPrepared = false
            try {
                val profile = TunnelEnrollmentProfile(
                    KetControlApi.normalizeBaseUrl(serverUrl),
                    KetControlApi.validateAccessCode(accessCode),
                    preferredProtocol,
                )
                val store = AndroidTunnelCredentialStore.get(appContext)
                val preparedSpec = DurableTunnelSessionResolver(KetControlApi, store).resolveForApp(profile)
                spec = preparedSpec
                sessionPrepared = true
                val launched = synchronized(lifecycleLock) {
                    if (attempt != generation.get()) return@synchronized false
                    KetTunnelRuntime.offer(preparedSpec)
                    KetTunnelRuntime.publish(
                        TunnelSnapshot(
                            phase = TunnelPhase.Connecting,
                            node = preparedSpec.node,
                            accessExpiresAtEpochSeconds = preparedSpec.accessExpiresAtEpochSeconds,
                            message = "Starting protected route...",
                        ),
                    )
                    ContextCompat.startForegroundService(
                        appContext,
                        Intent(appContext, KetVpnService::class.java)
                            .setAction(KetVpnService.ACTION_START)
                            .putExtra(KetVpnService.EXTRA_APP_STARTED, true),
                    )
                    true
                }
                if (!launched) {
                    runCatching { store.clearSession() }
                    runCatching {
                        KetControlApi.release(preparedSpec.controlEndpoint, preparedSpec.sessionToken)
                    }
                }
            } catch (error: Exception) {
                KetTunnelRuntime.clearPending()
                if (spec != null) runCatching {
                    KetControlApi.release(spec.controlEndpoint, spec.sessionToken)
                }
                if (sessionPrepared) runCatching { AndroidTunnelCredentialStore.get(appContext).clearSession() }
                if (attempt == generation.get()) {
                    KetTunnelRuntime.publish(
                        TunnelSnapshot(
                            phase = TunnelPhase.Failed,
                            message = error.message ?: "Unable to start Ket",
                        ),
                    )
                }
            }
        }
    }

    fun disconnect(context: Context) {
        val (pending, phase) = synchronized(lifecycleLock) {
            generation.incrementAndGet()
            KetTunnelRuntime.clearPending() to KetTunnelRuntime.snapshot().phase
        }
        if (pending != null) {
            executor.execute {
                runCatching { KetControlApi.release(pending.controlEndpoint, pending.sessionToken) }
                runCatching { AndroidTunnelCredentialStore.get(context.applicationContext).clearSession() }
            }
        }
        if (
            phase == TunnelPhase.Connecting ||
            phase == TunnelPhase.Reconnecting ||
            phase == TunnelPhase.Connected ||
            phase == TunnelPhase.Stopping
        ) {
            KetTunnelRuntime.update { it.copy(phase = TunnelPhase.Stopping, message = "Stopping protected route...") }
            context.applicationContext.startService(
                Intent(context.applicationContext, KetVpnService::class.java).setAction(KetVpnService.ACTION_STOP),
            )
        } else {
            runCatching { AndroidTunnelCredentialStore.get(context.applicationContext).clearSession() }
            KetTunnelRuntime.publish(TunnelSnapshot())
        }
    }
}
