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
                AndroidConnectionDiagnostics.begin(preferredProtocol)
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
                AndroidConnectionDiagnostics.record(
                    stage = "control_enrollment",
                    outcome = DiagnosticOutcome.Succeeded,
                    phase = TunnelPhase.Enrolling,
                    protocol = preferredProtocol,
                )
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
                    AndroidConnectionDiagnostics.record(
                        stage = "service_handoff",
                        outcome = DiagnosticOutcome.Succeeded,
                        phase = TunnelPhase.Connecting,
                        protocol = preferredProtocol,
                    )
                    true
                }
                if (!launched) {
                    val sessionCleanup = runCatching { store.clearSession() }
                    val release = runCatching {
                        KetControlApi.release(preparedSpec.controlEndpoint, preparedSpec.sessionToken)
                    }
                    AndroidConnectionDiagnostics.record(
                        stage = "cancelled_enrollment_cleanup",
                        outcome = if (sessionCleanup.isSuccess && release.isSuccess) {
                            DiagnosticOutcome.Succeeded
                        } else {
                            DiagnosticOutcome.Failed
                        },
                        phase = TunnelPhase.Stopping,
                        protocol = preferredProtocol,
                        code = if (sessionCleanup.isSuccess && release.isSuccess) null else "cleanup_incomplete",
                    )
                }
            } catch (error: Exception) {
                KetTunnelRuntime.clearPending()
                if (spec != null) runCatching {
                    KetControlApi.release(spec.controlEndpoint, spec.sessionToken)
                }
                if (sessionPrepared) runCatching { AndroidTunnelCredentialStore.get(appContext).clearSession() }
                AndroidConnectionDiagnostics.record(
                    stage = "control_enrollment",
                    outcome = DiagnosticOutcome.Failed,
                    phase = TunnelPhase.Failed,
                    protocol = preferredProtocol,
                    code = "enrollment_failed",
                    message = error.message,
                )
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
        AndroidConnectionDiagnostics.record(
            stage = "disconnect",
            outcome = DiagnosticOutcome.Started,
            phase = phase,
        )
        if (pending != null) {
            executor.execute {
                val release = runCatching { KetControlApi.release(pending.controlEndpoint, pending.sessionToken) }
                val cleared = runCatching { AndroidTunnelCredentialStore.get(context.applicationContext).clearSession() }
                AndroidConnectionDiagnostics.record(
                    stage = "pending_session_cleanup",
                    outcome = if (release.isSuccess && cleared.isSuccess) {
                        DiagnosticOutcome.Succeeded
                    } else {
                        DiagnosticOutcome.Failed
                    },
                    phase = TunnelPhase.Stopping,
                    code = if (release.isSuccess && cleared.isSuccess) null else "cleanup_incomplete",
                )
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
            val cleared = runCatching { AndroidTunnelCredentialStore.get(context.applicationContext).clearSession() }
            AndroidConnectionDiagnostics.record(
                stage = "local_session_cleanup",
                outcome = if (cleared.isSuccess) DiagnosticOutcome.Succeeded else DiagnosticOutcome.Failed,
                phase = TunnelPhase.Disconnected,
                code = if (cleared.isSuccess) null else "credential_cleanup_failed",
                message = cleared.exceptionOrNull()?.message,
            )
            KetTunnelRuntime.publish(TunnelSnapshot())
        }
    }
}
