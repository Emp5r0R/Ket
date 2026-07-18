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
    Connected,
    Stopping,
    Failed,
}

data class TunnelSnapshot(
    val phase: TunnelPhase = TunnelPhase.Disconnected,
    val node: String = "No server selected",
    val country: String = "",
    val message: String = "",
    val sentBytes: Long = 0,
    val receivedBytes: Long = 0,
    val onlineConnections: Int = 0,
    val capacityPercent: Double = 0.0,
    val handshakeLatencyMs: Long? = null,
    val transportName: String = "Auto",
)

internal class TunnelLaunchSpec(
    val controlEndpoint: String,
    val sessionToken: String,
    val node: String,
    val country: String,
    val transports: List<AndroidTransport>,
) {
    override fun toString(): String =
        "TunnelLaunchSpec(controlEndpoint=$controlEndpoint, sessionToken=[REDACTED], node=$node, country=$country, transports=$transports)"
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

    fun connect(context: Context, serverUrl: String, accessCode: String) {
        val attempt = generation.incrementAndGet()
        val appContext = context.applicationContext
        KetTunnelRuntime.publish(
            TunnelSnapshot(phase = TunnelPhase.Enrolling, message = "Authorizing device..."),
        )
        executor.execute {
            var result: EnrollmentResult? = null
            try {
                result = KetControlApi.enroll(serverUrl, accessCode, "Ket Android")
                if (attempt != generation.get()) {
                    runCatching { KetControlApi.release(serverUrl, result.token) }
                    return@execute
                }
                val spec = TunnelLaunchSpec(serverUrl, result.token, result.node, result.country, result.transports)
                KetTunnelRuntime.offer(spec)
                KetTunnelRuntime.publish(
                    TunnelSnapshot(
                        phase = TunnelPhase.Connecting,
                        node = result.node,
                        country = result.country,
                        message = "Starting protected route...",
                    ),
                )
                ContextCompat.startForegroundService(
                    appContext,
                    Intent(appContext, KetVpnService::class.java).setAction(KetVpnService.ACTION_START),
                )
            } catch (error: Exception) {
                KetTunnelRuntime.clearPending()
                if (result != null) runCatching { KetControlApi.release(serverUrl, result.token) }
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
        generation.incrementAndGet()
        val pending = KetTunnelRuntime.clearPending()
        if (pending != null) {
            executor.execute {
                runCatching { KetControlApi.release(pending.controlEndpoint, pending.sessionToken) }
            }
        }
        val phase = KetTunnelRuntime.snapshot().phase
        if (phase == TunnelPhase.Connecting || phase == TunnelPhase.Connected || phase == TunnelPhase.Stopping) {
            KetTunnelRuntime.update { it.copy(phase = TunnelPhase.Stopping, message = "Stopping protected route...") }
            context.applicationContext.startService(
                Intent(context.applicationContext, KetVpnService::class.java).setAction(KetVpnService.ACTION_STOP),
            )
        } else {
            KetTunnelRuntime.publish(TunnelSnapshot())
        }
    }
}
