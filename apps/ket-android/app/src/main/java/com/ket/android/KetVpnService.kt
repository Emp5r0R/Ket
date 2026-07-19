package com.ket.android

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.Manifest
import android.content.Intent
import android.content.pm.ServiceInfo
import android.content.pm.PackageManager
import android.net.IpPrefix
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import android.os.SystemClock
import android.system.Os
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import hev.htproxy.TProxyService
import java.io.File
import java.net.Inet4Address
import java.net.InetAddress
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/** Owns the Android TUN, maintained transport processes, and session lease lifecycle. */
class KetVpnService : VpnService() {
    private val worker = Executors.newSingleThreadExecutor()
    private val scheduler = Executors.newScheduledThreadPool(2)
    private val active = AtomicBoolean()
    private val stopping = AtomicBoolean()
    private val reconnecting = AtomicBoolean()
    private val transportHistory = AndroidTransportHistory()
    private val recoveryPolicy = AndroidRecoveryPolicy()
    private val bridge = TProxyService()
    @Volatile
    private var engine: AndroidTransportEngine? = null
    @Volatile
    private var activeTransportId: String? = null
    private var tun: ParcelFileDescriptor? = null
    private var bridgeConfig: File? = null
    @Volatile
    private var launchSpec: TunnelLaunchSpec? = null
    private var healthTask: ScheduledFuture<*>? = null
    private var telemetryTask: ScheduledFuture<*>? = null
    private var renewalFailures = 0

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            requestStop(null)
            return START_NOT_STICKY
        }
        if (intent?.action != ACTION_START || !active.compareAndSet(false, true)) return START_NOT_STICKY
        startForegroundCompat(notification("Starting protected route"))
        val spec = KetTunnelRuntime.take()
        if (spec == null) {
            requestStop("Tunnel authorization was not available")
            return START_NOT_STICKY
        }
        launchSpec = spec
        worker.execute { startTunnel(spec) }
        return START_NOT_STICKY
    }

    override fun onRevoke() {
        requestStop("Android revoked VPN permission")
        super.onRevoke()
    }

    override fun onDestroy() {
        healthTask?.cancel(true)
        telemetryTask?.cancel(true)
        cleanupLocal()
        scheduler.shutdownNow()
        worker.shutdownNow()
        super.onDestroy()
    }

    private fun startTunnel(spec: TunnelLaunchSpec) {
        try {
            val selected = startLocalRoute(spec, reconnectAttempt = 0)
            if (stopping.get()) return
            publishConnected(spec, selected)
            scheduleHealthChecks()
            scheduleTelemetry(spec)
        } catch (error: Exception) {
            requestStop(error.message ?: "Unable to establish the protected route")
        }
    }

    private fun startLocalRoute(spec: TunnelLaunchSpec, reconnectAttempt: Int): SelectedTransport {
        val failures = mutableListOf<String>()
        val candidates = transportHistory.rank(spec.transports, SystemClock.elapsedRealtime())
        for (transport in candidates) {
            if (stopping.get()) throw IllegalStateException("Tunnel stop was requested")
            KetTunnelRuntime.update {
                it.copy(
                    phase = if (reconnectAttempt > 0) TunnelPhase.Reconnecting else TunnelPhase.Connecting,
                    message = if (reconnectAttempt > 0) {
                        "Trying ${transport.displayName} recovery..."
                    } else {
                        "Starting ${transport.displayName}..."
                    },
                    transportName = transport.displayName,
                    reconnectAttempt = reconnectAttempt,
                )
            }
            val candidate = engineFor(transport) ?: continue
            try {
                val started = candidate.start(stopping::get)
                if (stopping.get()) throw IllegalStateException("Tunnel stop was requested")
                engine = candidate
                activeTransportId = transport.id
                establishVpnRoute(spec, candidate, started)
                transportHistory.recordSuccess(transport.id, started.handshakeLatencyMs)
                return SelectedTransport(candidate, started)
            } catch (error: Exception) {
                if (engine === candidate) {
                    cleanupLocal()
                } else {
                    runCatching { candidate.close() }
                }
                if (stopping.get()) throw IllegalStateException("Tunnel stop was requested")
                transportHistory.recordFailure(transport.id, SystemClock.elapsedRealtime())
                failures += "${transport.displayName}: ${error.message ?: "startup failed"}"
            }
        }
        throw IllegalStateException(
            failures.joinToString("; ").ifBlank { "No supported transport could start" },
        )
    }

    private fun engineFor(transport: AndroidTransport): AndroidTransportEngine? = when (transport) {
        is HysteriaTransport -> AndroidHysteriaEngine(this, transport)
        is RealityTransport -> AndroidXrayEngine(this, transport)
        else -> null
    }

    private fun establishVpnRoute(
        spec: TunnelLaunchSpec,
        selectedEngine: AndroidTransportEngine,
        selected: AndroidEngineStarted,
    ) {
        val builder = Builder()
            .setSession("Ket - ${spec.node}")
            .setMtu(TUN_MTU)
            .setBlocking(false)
            .addAddress(TUN_IPV4, 32)
            .addAddress(TUN_IPV6, 128)
            .addDnsServer("1.1.1.1")
            .addDnsServer("2606:4700:4700::1111")
            .apply { if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) setMetered(false) }
        addVpnRoutes(builder, selected.bypassAddress)
        val established = builder.establish()
            ?: throw IllegalStateException("Android could not establish the VPN interface")
        tun = established
        bridgeConfig = writeBridgeConfig(EngineConfig.tunToSocks(selected.socksPort))
        bridge.start(bridgeConfig!!.absolutePath, established.fd)
        Thread.sleep(BRIDGE_SETTLE_MILLIS)
        if (!selectedEngine.isAlive()) {
            throw IllegalStateException("${selectedEngine.displayName} stopped while attaching the VPN interface")
        }
    }

    private fun publishConnected(spec: TunnelLaunchSpec, selected: SelectedTransport) {
        reconnecting.set(false)
        KetTunnelRuntime.update {
            it.copy(
                phase = TunnelPhase.Connected,
                node = spec.node,
                country = spec.country,
                message = "Protected with ${selected.engine.displayName}",
                handshakeLatencyMs = selected.started.handshakeLatencyMs,
                transportName = selected.engine.displayName,
                reconnectAttempt = 0,
            )
        }
        updateForegroundNotification("Protected with ${selected.engine.displayName}")
    }

    private fun scheduleHealthChecks() {
        healthTask = scheduler.scheduleWithFixedDelay(
            {
                val currentEngine = engine
                if (!stopping.get() && currentEngine != null && !currentEngine.isAlive()) {
                    requestRecovery("${currentEngine.displayName} stopped unexpectedly")
                }
            },
            2,
            2,
            TimeUnit.SECONDS,
        )
    }

    private fun recoverTunnel(
        spec: TunnelLaunchSpec,
        failedTransportId: String?,
        failureReason: String,
    ) {
        if (failedTransportId != null) {
            transportHistory.recordFailure(failedTransportId, SystemClock.elapsedRealtime())
        }
        cleanupLocal()
        var lastFailure = failureReason
        for (attempt in 1..recoveryPolicy.maximumReconnectRounds) {
            if (stopping.get()) {
                reconnecting.set(false)
                return
            }
            KetTunnelRuntime.update {
                it.copy(
                    phase = TunnelPhase.Reconnecting,
                    message = "Restoring protected route...",
                    reconnectAttempt = attempt,
                )
            }
            updateForegroundNotification("Restoring protected route")
            try {
                val selected = startLocalRoute(spec, reconnectAttempt = attempt)
                if (stopping.get()) {
                    cleanupLocal()
                    reconnecting.set(false)
                    return
                }
                publishConnected(spec, selected)
                return
            } catch (error: Exception) {
                lastFailure = error.message ?: "Transport recovery failed"
            }
            if (attempt < recoveryPolicy.maximumReconnectRounds) {
                try {
                    Thread.sleep(RECONNECT_RETRY_MILLIS * attempt)
                } catch (_: InterruptedException) {
                    Thread.currentThread().interrupt()
                    break
                }
            }
        }
        requestStop("Unable to restore the protected route: $lastFailure")
        reconnecting.set(false)
    }

    private fun requestRecovery(failureReason: String) {
        if (stopping.get() || !reconnecting.compareAndSet(false, true)) return
        val currentEngine = engine
        val spec = launchSpec
        if (currentEngine == null || spec == null) {
            reconnecting.set(false)
            requestStop(failureReason)
            return
        }
        val failedTransportId = activeTransportId
        worker.execute {
            recoverTunnel(
                spec = spec,
                failedTransportId = failedTransportId,
                failureReason = failureReason,
            )
        }
    }

    private fun scheduleTelemetry(spec: TunnelLaunchSpec) {
        telemetryTask = scheduler.scheduleWithFixedDelay(
            {
                if (stopping.get()) return@scheduleWithFixedDelay
                try {
                    KetControlApi.renew(spec.controlEndpoint, spec.sessionToken)
                    renewalFailures = 0
                } catch (_: Exception) {
                    renewalFailures += 1
                    when (recoveryPolicy.leaseFailureAction(renewalFailures)) {
                        AndroidLeaseFailureAction.Wait -> Unit
                        AndroidLeaseFailureAction.Recover -> requestRecovery(
                            "The protected route stopped reaching the server",
                        )
                        AndroidLeaseFailureAction.Stop -> requestStop(
                            "The server session could not be renewed after transport recovery",
                        )
                    }
                    return@scheduleWithFixedDelay
                }
                val remote = runCatching {
                    KetControlApi.status(spec.controlEndpoint, spec.sessionToken)
                }.getOrNull()
                val local = if (reconnecting.get()) {
                    null
                } else {
                    runCatching { bridge.stats() }.getOrNull()
                }
                if (remote != null || local != null) {
                    KetTunnelRuntime.update {
                        it.copy(
                            node = remote?.node ?: it.node,
                            sentBytes = local?.getOrNull(1) ?: remote?.sent ?: it.sentBytes,
                            receivedBytes = local?.getOrNull(3) ?: remote?.received ?: it.receivedBytes,
                            onlineConnections = remote?.online ?: it.onlineConnections,
                            capacityPercent = remote?.capacity ?: it.capacityPercent,
                        )
                    }
                }
            },
            1,
            10,
            TimeUnit.SECONDS,
        )
    }

    private fun requestStop(failure: String?) {
        if (!stopping.compareAndSet(false, true)) return
        worker.execute { stopTunnel(failure) }
    }

    private fun stopTunnel(failure: String?) {
        healthTask?.cancel(true)
        telemetryTask?.cancel(true)
        reconnecting.set(false)
        cleanupLocal()
        val spec = launchSpec
        launchSpec = null
        if (spec != null) runCatching { KetControlApi.release(spec.controlEndpoint, spec.sessionToken) }
        if (failure == null) {
            KetTunnelRuntime.publish(TunnelSnapshot())
        } else {
            KetTunnelRuntime.publish(
                TunnelSnapshot(
                    phase = TunnelPhase.Failed,
                    node = spec?.node ?: KetTunnelRuntime.snapshot().node,
                    country = spec?.country ?: KetTunnelRuntime.snapshot().country,
                    message = failure,
                ),
            )
        }
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    @Synchronized
    private fun cleanupLocal() {
        runCatching { bridge.stop() }
        runCatching { tun?.close() }
        tun = null
        runCatching { engine?.close() }
        engine = null
        activeTransportId = null
        bridgeConfig?.delete()
        bridgeConfig = null
    }

    private fun writeBridgeConfig(document: String): File {
        return File.createTempFile("tun-to-socks-", ".yml", cacheDir).also {
            it.writeText(document)
            Os.chmod(it.absolutePath, 0x180) // 0600
        }
    }

    private fun addVpnRoutes(builder: Builder, bypassAddress: InetAddress?) {
        if (bypassAddress == null) {
            builder.addRoute("0.0.0.0", 0)
            builder.addRoute("::", 0)
            return
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            builder.addRoute("0.0.0.0", 0)
            builder.addRoute("::", 0)
            builder.excludeRoute(IpPrefix(bypassAddress, if (bypassAddress is Inet4Address) 32 else 128))
            return
        }
        if (bypassAddress is Inet4Address) {
            routesExcluding(bypassAddress).forEach {
                builder.addRoute(requireNotNull(it.address.hostAddress), it.prefixLength)
            }
            builder.addRoute("::", 0)
        } else {
            builder.addRoute("0.0.0.0", 0)
            routesExcluding(bypassAddress).forEach {
                builder.addRoute(requireNotNull(it.address.hostAddress), it.prefixLength)
            }
        }
    }

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            NOTIFICATION_CHANNEL,
            "Ket VPN connection",
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = "Current Ket VPN connection state"
            setShowBadge(false)
        }
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    private fun notification(message: String): Notification {
        val activity = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val stop = PendingIntent.getService(
            this,
            1,
            Intent(this, KetVpnService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return NotificationCompat.Builder(this, NOTIFICATION_CHANNEL)
            .setSmallIcon(com.ket.android.R.drawable.ket_mark)
            .setContentTitle("Ket")
            .setContentText(message)
            .setContentIntent(activity)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .addAction(0, "Disconnect", stop)
            .build()
    }

    private fun startForegroundCompat(notification: Notification) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    private fun updateForegroundNotification(message: String) {
        if (
            Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        ) {
            getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification(message))
        }
    }

    companion object {
        const val ACTION_START = "com.ket.android.action.START"
        const val ACTION_STOP = "com.ket.android.action.STOP"
        private const val NOTIFICATION_CHANNEL = "ket-vpn"
        private const val NOTIFICATION_ID = 3712
        private const val TUN_MTU = 1400
        private const val TUN_IPV4 = "198.18.0.1"
        private const val TUN_IPV6 = "fc00::1"
        private const val RECONNECT_RETRY_MILLIS = 3_000L
        private const val BRIDGE_SETTLE_MILLIS = 250L
    }
}

private data class SelectedTransport(
    val engine: AndroidTransportEngine,
    val started: AndroidEngineStarted,
)

internal data class RoutePrefix(val address: InetAddress, val prefixLength: Int)

internal fun routesExcluding(address: InetAddress): List<RoutePrefix> {
    val source = address.address
    val bitCount = source.size * 8
    return (0 until bitCount).map { bit ->
        val sibling = source.copyOf()
        val byteIndex = bit / 8
        val bitMask = 1 shl (7 - bit % 8)
        sibling[byteIndex] = (sibling[byteIndex].toInt() xor bitMask).toByte()
        for (remaining in bit + 1 until bitCount) {
            val index = remaining / 8
            val mask = 1 shl (7 - remaining % 8)
            sibling[index] = (sibling[index].toInt() and mask.inv()).toByte()
        }
        RoutePrefix(InetAddress.getByAddress(sibling), bit + 1)
    }
}
