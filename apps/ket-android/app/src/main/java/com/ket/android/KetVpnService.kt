package com.ket.android

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.Manifest
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.net.IpPrefix
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import android.os.PowerManager
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
import com.ket.android.connection.ConnectionOptions
import com.ket.android.connection.ConnectionOptionsStore
import com.ket.android.connection.TransportFallbackNote
import com.ket.android.connection.TransportFallbacks
import com.ket.android.splittunnel.InstalledApps
import com.ket.android.splittunnel.SplitTunnelRules
import com.ket.android.splittunnel.SplitTunnelStore
import com.ket.android.splittunnel.VpnAppRules

/** Owns the Android TUN, maintained transport processes, and session lease lifecycle. */
class KetVpnService : VpnService() {
    private val worker = Executors.newSingleThreadExecutor()
    private val scheduler = Executors.newScheduledThreadPool(2)
    private val lifecycle = AndroidVpnServiceLifecycle()
    private val reconnecting = AtomicBoolean()
    private val leaseRefreshGate = AndroidLeaseRefreshGate()
    private val transportHistory = AndroidTransportHistory()
    private val recoveryPolicy = AndroidRecoveryPolicy()
    private val underlyingNetworks = UnderlyingNetworkTracker<Network>()
    private val networkChangeVerifier = NetworkChangeVerifier(
        routeWorks = { runCatching { AndroidRoutedInternetProbe.verify(lifecycle::isStopping) }.isSuccess },
    )
    private val bridge = TProxyService()
    private lateinit var connectivityManager: ConnectivityManager
    private lateinit var powerManager: PowerManager
    private lateinit var credentialStore: TunnelCredentialStore
    private val networkCallback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            scheduleNetworkChangeRecovery(underlyingNetworks.onAvailable(network))
        }

        override fun onLost(network: Network) {
            scheduleNetworkChangeRecovery(underlyingNetworks.onLost(network))
        }
    }
    private val idleModeReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if (
                intent?.action == PowerManager.ACTION_DEVICE_IDLE_MODE_CHANGED &&
                !powerManager.isDeviceIdleMode
            ) {
                requestImmediateTelemetryRefresh()
            }
        }
    }
    @Volatile
    private var engine: AndroidTransportEngine? = null
    @Volatile
    private var activeTransportId: String? = null
    private val vpnRoute = FailClosedVpnRouteGuard<ParcelFileDescriptor>()
    private var bridgeConfig: File? = null
    @Volatile
    private var bridgeActive = false
    @Volatile
    private var launchSpec: TunnelLaunchSpec? = null
    private var transportAddresses: Map<String, InetAddress> = emptyMap()
    private var healthTask: ScheduledFuture<*>? = null
    private var telemetryTask: ScheduledFuture<*>? = null
    private var networkChangeTask: ScheduledFuture<*>? = null
    private var networkCallbackRegistered = false
    private var idleModeReceiverRegistered = false
    private var renewalFailures = 0
    @Volatile
    private var renewAtEpochSeconds = Long.MAX_VALUE

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        connectivityManager = getSystemService(ConnectivityManager::class.java)
        powerManager = getSystemService(PowerManager::class.java)
        credentialStore = AndroidTunnelCredentialStore.get(this)
        connectivityManager.registerNetworkCallback(
            NetworkRequest.Builder()
                .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
                .addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN)
                .build(),
            networkCallback,
        )
        networkCallbackRegistered = true
        ContextCompat.registerReceiver(
            this,
            idleModeReceiver,
            IntentFilter(PowerManager.ACTION_DEVICE_IDLE_MODE_CHANGED),
            ContextCompat.RECEIVER_NOT_EXPORTED,
        )
        idleModeReceiverRegistered = true
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            requestStop(null)
            return START_NOT_STICKY
        }
        if (!lifecycle.tryStart()) return START_REDELIVER_INTENT
        startForegroundCompat(notification("Starting protected route"))
        val pending = if (intent?.getBooleanExtra(EXTRA_APP_STARTED, false) == true) {
            KetTunnelRuntime.take()
        } else {
            null
        }
        if (pending == null) {
            AndroidConnectionDiagnostics.begin(null, source = "restore")
            KetTunnelRuntime.publish(
                TunnelSnapshot(
                    phase = TunnelPhase.Connecting,
                    message = "Restoring saved protected route...",
                ),
            )
        }
        worker.execute {
            try {
                val spec = DurableTunnelSessionResolver(KetControlApi, credentialStore).resolve(pending)
                launchSpec = spec
                startTunnel(spec)
            } catch (error: Exception) {
                requestStop(error.message ?: "Unable to restore tunnel authorization")
            }
        }
        return START_REDELIVER_INTENT
    }

    override fun onRevoke() {
        // VpnService's default implementation stops immediately; our worker must release the lease first.
        requestStop("Android revoked VPN permission")
    }

    override fun onDestroy() {
        val terminalSnapshot = lifecycle.destroy()
        healthTask?.cancel(true)
        telemetryTask?.cancel(true)
        networkChangeTask?.cancel(true)
        if (networkCallbackRegistered) {
            connectivityManager.unregisterNetworkCallback(networkCallback)
            networkCallbackRegistered = false
        }
        if (idleModeReceiverRegistered) {
            unregisterReceiver(idleModeReceiver)
            idleModeReceiverRegistered = false
        }
        cleanupLocal()
        terminalSnapshot?.let(KetTunnelRuntime::publish)
        scheduler.shutdownNow()
        worker.shutdownNow()
        super.onDestroy()
    }

    private fun startTunnel(spec: TunnelLaunchSpec) {
        try {
            AndroidConnectionDiagnostics.record(
                stage = "tunnel_start",
                outcome = DiagnosticOutcome.Started,
                phase = TunnelPhase.Connecting,
                protocol = spec.preferredProtocol,
            )
            transportAddresses = resolveTransportAddresses(spec.transports)
            val selected = startLocalRoute(spec, reconnectAttempt = 0)
            if (lifecycle.isStopping()) return
            publishConnected(spec, selected)
            scheduleHealthChecks()
            scheduleTelemetry()
        } catch (error: Exception) {
            AndroidConnectionDiagnostics.record(
                stage = "tunnel_start",
                outcome = DiagnosticOutcome.Failed,
                phase = TunnelPhase.Failed,
                protocol = spec.preferredProtocol,
                code = "tunnel_start_failed",
                message = error.message,
            )
            requestStop(error.message ?: "Unable to establish the protected route")
        }
    }

    private fun startLocalRoute(spec: TunnelLaunchSpec, reconnectAttempt: Int): SelectedTransport {
        val failures = mutableListOf<String>()
        val options = runCatching { ConnectionOptionsStore(this).load() }.getOrDefault(ConnectionOptions())
        val offered = spec.transports.map(AndroidTransport::protocol).distinct()
        // A recovery keeps the reason the previous transport was left.
        KetTunnelRuntime.update {
            it.copy(
                fallbacks = if (reconnectAttempt == 0) emptyList() else it.fallbacks,
                availableProtocols = offered,
            )
        }
        TransportFallbacks.preferredUnavailable(spec.transports, spec.preferredProtocol)?.let {
            throw IllegalStateException(it)
        }
        val candidates = transportHistory.rank(
            spec.transports,
            SystemClock.elapsedRealtime(),
            spec.preferredProtocol,
        )
        for (transport in candidates) {
            if (lifecycle.isStopping()) throw IllegalStateException("Tunnel stop was requested")
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
            AndroidConnectionDiagnostics.record(
                stage = if (reconnectAttempt > 0) "transport_recovery" else "transport_start",
                outcome = DiagnosticOutcome.Started,
                phase = if (reconnectAttempt > 0) TunnelPhase.Reconnecting else TunnelPhase.Connecting,
                protocol = transport.protocol,
                transport = transport.id,
            )
            val candidate = engineFor(spec, transport, options)
            if (candidate == null) {
                transportHistory.recordFailure(transport.id, SystemClock.elapsedRealtime())
                failures += "${transport.displayName}: endpoint did not resolve before VPN routing"
                noteFallback(transport.displayName, "endpoint did not resolve before VPN routing")
                AndroidConnectionDiagnostics.record(
                    stage = "transport_resolution",
                    outcome = DiagnosticOutcome.Failed,
                    phase = if (reconnectAttempt > 0) TunnelPhase.Reconnecting else TunnelPhase.Connecting,
                    protocol = transport.protocol,
                    transport = transport.id,
                    code = "endpoint_resolution_failed",
                )
                continue
            }
            try {
                val started = candidate.start(lifecycle::isStopping)
                if (lifecycle.isStopping()) throw IllegalStateException("Tunnel stop was requested")
                engine = candidate
                activeTransportId = transport.id
                establishVpnRoute(spec, candidate, started)
                transportHistory.recordSuccess(transport.id, started.handshakeLatencyMs)
                AndroidConnectionDiagnostics.record(
                    stage = if (reconnectAttempt > 0) "transport_recovery" else "transport_start",
                    outcome = DiagnosticOutcome.Succeeded,
                    phase = TunnelPhase.Connected,
                    protocol = transport.protocol,
                    transport = transport.id,
                )
                return SelectedTransport(candidate, started)
            } catch (error: Exception) {
                if (engine === candidate) {
                    stopTransportAndBridge()
                } else {
                    runCatching { candidate.close() }
                }
                if (lifecycle.isStopping()) throw IllegalStateException("Tunnel stop was requested")
                transportHistory.recordFailure(transport.id, SystemClock.elapsedRealtime())
                failures += "${transport.displayName}: ${error.message ?: "startup failed"}"
                noteFallback(transport.displayName, error.message ?: "startup failed")
                AndroidConnectionDiagnostics.record(
                    stage = if (reconnectAttempt > 0) "transport_recovery" else "transport_start",
                    outcome = DiagnosticOutcome.Failed,
                    phase = if (reconnectAttempt > 0) TunnelPhase.Reconnecting else TunnelPhase.Connecting,
                    protocol = transport.protocol,
                    transport = transport.id,
                    code = "transport_start_failed",
                    message = error.message,
                )
            }
        }
        throw IllegalStateException(
            failures.joinToString("; ").ifBlank { "No supported transport could start" },
        )
    }

    private fun engineFor(
        spec: TunnelLaunchSpec,
        transport: AndroidTransport,
        options: ConnectionOptions,
    ): AndroidTransportEngine? = when (transport) {
        is HysteriaTransport -> AndroidHysteriaEngine(this, transport)
        is ShadowsocksTransport -> transportAddresses[transport.id]?.let {
            AndroidShadowsocksEngine(this, transport, it)
        }
        is WireGuardTlsTransport -> transportAddresses[transport.id]?.let {
            AndroidWireGuardTlsEngine(this, transport, it)
        }
        is AndroidXrayTransport -> transportAddresses[transport.id]?.let {
            AndroidXrayEngine(this, transport, it, options.splitTlsHello)
        }
        is OpenVpnStunnelTransport -> transportAddresses[transport.id]?.let { resolved ->
            AndroidOpenVpnEngine(this, transport, resolved) { request ->
                establishOpenVpnRoute(spec, resolved, request)
            }
        }
        else -> null
    }

    private fun establishVpnRoute(
        spec: TunnelLaunchSpec,
        selectedEngine: AndroidTransportEngine,
        selected: AndroidEngineStarted,
    ) {
        if (selected.route == AndroidEngineRoute.NativeTun) {
            require(vpnRoute.isActive()) { "OpenVPN did not attach its Android tunnel interface" }
            require(selectedEngine.isAlive()) { "OpenVPN stopped while attaching the VPN interface" }
            bridgeActive = false
            AndroidRoutedInternetProbe.verify(lifecycle::isStopping)
            return
        }
        val socks = selected.route as AndroidEngineRoute.Socks
        val bypassAddresses =
            (transportAddresses.values + listOfNotNull(selected.bypassAddress)).distinct()
        val builder = Builder()
            .setSession("Ket - ${spec.node.displayName}")
            .setMtu(TUN_MTU)
            .setBlocking(false)
            .addAddress(TUN_IPV4, 32)
            .addAddress(TUN_IPV6, 128)
            .apply { if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) setMetered(false) }
        // Do not call allowBypass(): apps must not bind around the active VPN.
        builder.addDnsServer(AndroidVpnDnsPolicy.server())
        addVpnRoutes(builder, bypassAddresses)
        applySplitTunnel(builder)
        val established = builder.establish()
            ?: throw IllegalStateException("Android could not establish the VPN interface")
        vpnRoute.replace(established)
        bridgeConfig = writeBridgeConfig(EngineConfig.tunToSocks(socks.port))
        bridge.start(bridgeConfig!!.absolutePath, established.fd)
        bridgeActive = true
        Thread.sleep(BRIDGE_SETTLE_MILLIS)
        if (!selectedEngine.isAlive()) {
            throw IllegalStateException("${selectedEngine.displayName} stopped while attaching the VPN interface")
        }
        AndroidRoutedInternetProbe.verify(lifecycle::isStopping)
    }

    private fun establishOpenVpnRoute(
        spec: TunnelLaunchSpec,
        resolvedEndpoint: InetAddress,
        request: OpenVpnTunRequest,
    ): ParcelFileDescriptor {
        val bypassAddresses = (transportAddresses.values + resolvedEndpoint).distinct()
        val builder = Builder()
            .setSession("Ket - ${spec.node.displayName}")
            .setMtu(request.mtu)
            .setBlocking(false)
            .addAddress(requireNotNull(request.ipv4Address.hostAddress), request.ipv4PrefixLength)
            .apply {
                val ipv6 = request.ipv6Address
                val prefix = request.ipv6PrefixLength
                if (ipv6 != null && prefix != null) {
                    addAddress(requireNotNull(ipv6.hostAddress), prefix)
                } else {
                    // Route IPv6 into the TUN even when the server intentionally blocks it.
                    addAddress(TUN_IPV6, 128)
                }
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) setMetered(false)
            }
        request.dnsServers.forEach(builder::addDnsServer)
        addVpnRoutes(builder, bypassAddresses)
        applySplitTunnel(builder)
        val established = builder.establish()
            ?: throw IllegalStateException("Android could not establish the OpenVPN interface")
        vpnRoute.replace(established)
        return ParcelFileDescriptor.dup(established.fileDescriptor)
    }

    private fun publishConnected(spec: TunnelLaunchSpec, selected: SelectedTransport) {
        reconnecting.set(false)
        renewalFailures = 0
        renewAtEpochSeconds = AndroidLeaseRenewalSchedule.nextRenewalEpochSeconds(
            System.currentTimeMillis() / 1_000,
            spec.sessionExpiresAtEpochSeconds,
        )
        underlyingNetworks.markConnected()
        KetTunnelRuntime.update {
            it.copy(
                phase = TunnelPhase.Connected,
                node = spec.node,
                accessExpiresAtEpochSeconds = spec.accessExpiresAtEpochSeconds,
                message = "Liberated with ${selected.engine.displayName}",
                handshakeLatencyMs = selected.started.handshakeLatencyMs,
                transportName = selected.engine.displayName,
                reconnectAttempt = 0,
            )
        }
        updateForegroundNotification("Liberated with ${selected.engine.displayName}")
        AndroidConnectionDiagnostics.record(
            stage = "full_route_verification",
            outcome = DiagnosticOutcome.Succeeded,
            phase = TunnelPhase.Connected,
            protocol = spec.preferredProtocol,
            transport = activeTransportId,
        )
    }

    private fun scheduleHealthChecks() {
        healthTask = scheduler.scheduleWithFixedDelay(
            {
                val currentEngine = engine
                if (!lifecycle.isStopping() && currentEngine != null && !currentEngine.isAlive()) {
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
        blameTransport: Boolean = true,
    ) {
        AndroidConnectionDiagnostics.record(
            stage = "route_recovery",
            outcome = DiagnosticOutcome.Started,
            phase = TunnelPhase.Reconnecting,
            protocol = spec.preferredProtocol,
            transport = failedTransportId,
            message = failureReason,
        )
        // After a network change the network, not the transport, was at fault: it is tried
        // again first instead of being cooled down.
        if (failedTransportId != null && blameTransport) {
            transportHistory.recordFailure(failedTransportId, SystemClock.elapsedRealtime())
            spec.transports.firstOrNull { it.id == failedTransportId }?.let {
                noteFallback(it.displayName, failureReason)
            }
        }
        stopTransportAndBridge()
        var lastFailure = failureReason
        for (attempt in 1..recoveryPolicy.maximumReconnectRounds) {
            if (lifecycle.isStopping()) {
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
                if (lifecycle.isStopping()) {
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
        AndroidConnectionDiagnostics.record(
            stage = "route_recovery",
            outcome = DiagnosticOutcome.Failed,
            phase = TunnelPhase.Failed,
            protocol = spec.preferredProtocol,
            transport = failedTransportId,
            code = "recovery_exhausted",
            message = lastFailure,
        )
        reconnecting.set(false)
    }

    private fun noteFallback(transportName: String, reason: String) {
        KetTunnelRuntime.update {
            it.copy(fallbacks = TransportFallbacks.appended(it.fallbacks, TransportFallbackNote(transportName, reason)))
        }
    }

    private fun requestRecovery(failureReason: String, blameTransport: Boolean = true) {
        if (lifecycle.isStopping() || !reconnecting.compareAndSet(false, true)) return
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
                blameTransport = blameTransport,
            )
        }
    }

    private fun requestSessionReauthorization(failureReason: String) {
        if (lifecycle.isStopping() || !reconnecting.compareAndSet(false, true)) return
        worker.execute {
            AndroidConnectionDiagnostics.record(
                stage = "session_reauthorization",
                outcome = DiagnosticOutcome.Started,
                phase = TunnelPhase.Reconnecting,
                transport = activeTransportId,
                code = "authorization_lost",
                message = failureReason,
            )
            KetTunnelRuntime.update {
                it.copy(
                    phase = TunnelPhase.Reconnecting,
                    message = "Refreshing server authorization...",
                    reconnectAttempt = 1,
                )
            }
            try {
                val saved = credentialStore.load()
                    ?: throw IllegalStateException("The saved access profile is unavailable")
                if (saved.accessExpired()) {
                    credentialStore.clearAll()
                    throw IllegalStateException("Access time has expired")
                }
                credentialStore.clearSession()
                val replacement = DurableTunnelSessionResolver(KetControlApi, credentialStore)
                    .resolveForApp(saved.profile)
                launchSpec = replacement
                if (lifecycle.isStopping()) return@execute
                val replacementAddresses = resolveTransportAddresses(replacement.transports)
                stopTransportAndBridge()
                transportAddresses = replacementAddresses
                val selected = startLocalRoute(replacement, reconnectAttempt = 1)
                if (lifecycle.isStopping()) return@execute
                publishConnected(replacement, selected)
                AndroidConnectionDiagnostics.record(
                    stage = "session_reauthorization",
                    outcome = DiagnosticOutcome.Succeeded,
                    phase = TunnelPhase.Connected,
                    protocol = replacement.preferredProtocol,
                    transport = activeTransportId,
                )
            } catch (error: Exception) {
                AndroidConnectionDiagnostics.record(
                    stage = "session_reauthorization",
                    outcome = DiagnosticOutcome.Failed,
                    phase = TunnelPhase.Failed,
                    transport = activeTransportId,
                    code = "reauthorization_failed",
                    message = error.message,
                )
                reconnecting.set(false)
                if (!lifecycle.isStopping()) {
                    requestStop(error.message ?: "The server session could not be refreshed")
                }
            }
        }
    }

    @Synchronized
    private fun scheduleNetworkChangeRecovery(networkSetChanged: Boolean) {
        if (!networkSetChanged || !underlyingNetworks.isConnected() || scheduler.isShutdown) return
        networkChangeTask?.cancel(false)
        networkChangeTask = scheduler.schedule(
            {
                if (!lifecycle.isStopping() && underlyingNetworks.consumeRecoveryRequired()) {
                    verifyAfterNetworkChange()
                }
            },
            NETWORK_CHANGE_DEBOUNCE_MILLIS,
            TimeUnit.MILLISECONDS,
        )
    }

    /**
     * Keeps a route that survived the move as it is, with nothing shown; replaces one that did not.
     * Runs on the worker, so it never overlaps a recovery.
     */
    private fun verifyAfterNetworkChange() {
        if (reconnecting.get()) return
        runCatching {
            worker.execute {
                if (lifecycle.isStopping() || reconnecting.get() || engine == null) return@execute
                val survived = networkChangeVerifier.routeSurvived(lifecycle::isStopping)
                if (lifecycle.isStopping()) return@execute
                AndroidConnectionDiagnostics.record(
                    stage = "network_change",
                    outcome = if (survived) DiagnosticOutcome.Succeeded else DiagnosticOutcome.Failed,
                    phase = if (survived) TunnelPhase.Connected else TunnelPhase.Reconnecting,
                    transport = activeTransportId,
                    code = if (survived) "route_kept" else "route_lost",
                )
                if (!survived) {
                    requestRecovery("The route stopped working after the network changed", blameTransport = false)
                }
            }
        }
    }

    private fun scheduleTelemetry() {
        telemetryTask = scheduler.scheduleWithFixedDelay(
            { launchSpec?.let(::refreshTelemetry) },
            1,
            10,
            TimeUnit.SECONDS,
        )
    }

    private fun requestImmediateTelemetryRefresh() {
        if (launchSpec == null) return
        if (lifecycle.isStopping() || scheduler.isShutdown) return
        runCatching { scheduler.execute { launchSpec?.let(::refreshTelemetry) } }
    }

    private fun refreshTelemetry(spec: TunnelLaunchSpec) {
        if (!leaseRefreshGate.tryStart(lifecycle.isStopping(), powerManager.isDeviceIdleMode)) return
        try {
            val nowEpochSeconds = System.currentTimeMillis() / 1_000
            val renewalDue = AndroidLeaseRenewalSchedule.isDue(nowEpochSeconds, renewAtEpochSeconds)
            val remote: SessionTelemetry
            try {
                remote = if (renewalDue) {
                    KetControlApi.renewStatus(spec.controlEndpoint, spec.sessionToken, spec.controlNode)
                } else {
                    KetControlApi.status(spec.controlEndpoint, spec.sessionToken, spec.controlNode)
                }
                confirmSessionLease(spec, remote, nowEpochSeconds, renewalDue)
                renewalFailures = 0
            } catch (error: Exception) {
                val requiredUpdate = (error as? KetControlException)?.requiredUpdate
                if (requiredUpdate != null) {
                    // The server no longer serves this app version; renewing again cannot help.
                    AndroidConnectionDiagnostics.record(
                        stage = "lease_renewal",
                        outcome = DiagnosticOutcome.Failed,
                        phase = KetTunnelRuntime.snapshot().phase,
                        protocol = spec.preferredProtocol,
                        transport = activeTransportId,
                        code = CLIENT_UPDATE_REQUIRED_CODE,
                        message = error.message,
                    )
                    KetTunnelRuntime.update { it.copy(clientUpdate = requiredUpdate) }
                    requestStop(error.message ?: "Update Ket to keep using this server")
                    return
                }
                val authorizationLost = (error as? KetControlException)?.authorizationLost == true
                if (!authorizationLost && powerManager.isDeviceIdleMode) return
                renewalFailures += 1
                AndroidConnectionDiagnostics.record(
                    stage = "lease_renewal",
                    outcome = DiagnosticOutcome.Failed,
                    phase = KetTunnelRuntime.snapshot().phase,
                    protocol = spec.preferredProtocol,
                    transport = activeTransportId,
                    code = if (authorizationLost) "authorization_lost" else "renewal_failed",
                    message = error.message,
                )
                val routedInternetHealthy = if (
                    recoveryPolicy.shouldVerifyRoute(renewalFailures, authorizationLost)
                ) {
                    runCatching { AndroidRoutedInternetProbe.verify(lifecycle::isStopping) }.isSuccess.also {
                        AndroidConnectionDiagnostics.record(
                            stage = "routed_health_after_control_failure",
                            outcome = if (it) DiagnosticOutcome.Succeeded else DiagnosticOutcome.Failed,
                            phase = KetTunnelRuntime.snapshot().phase,
                            protocol = spec.preferredProtocol,
                            transport = activeTransportId,
                            code = if (it) "route_healthy" else "route_unavailable",
                        )
                    }
                } else {
                    null
                }
                when (
                    recoveryPolicy.leaseFailureAction(
                        renewalFailures,
                        authorizationLost,
                        routedInternetHealthy,
                        spec.accessExpiresAtEpochSeconds
                            ?.let { it <= System.currentTimeMillis() / 1_000 } == true,
                    )
                ) {
                    AndroidLeaseFailureAction.Wait -> Unit
                    AndroidLeaseFailureAction.Recover -> requestRecovery(
                        "The protected route stopped reaching the server",
                    )
                    AndroidLeaseFailureAction.Reauthorize -> requestSessionReauthorization(
                        "The server no longer recognizes the active session",
                    )
                    AndroidLeaseFailureAction.Stop -> requestStop(
                        if (authorizationLost) {
                            "The server session is no longer authorized"
                        } else {
                            "The server session could not be renewed after transport recovery"
                        },
                    )
                }
                return
            }
            if (lifecycle.isStopping()) return
            val local = if (reconnecting.get()) {
                null
            } else {
                engine?.trafficStats() ?: if (bridgeActive) {
                    runCatching { bridge.stats() }.getOrNull()
                } else {
                    null
                }
            }
            if (lifecycle.isStopping()) return
            val remoteTraffic = remote.takeIf(SessionTelemetry::available)
            KetTunnelRuntime.update {
                it.copy(
                    node = remote.node,
                    accessExpiresAtEpochSeconds = remote.accessExpiresAtEpochSeconds
                        ?: it.accessExpiresAtEpochSeconds,
                    sentBytes = local?.getOrNull(1) ?: remoteTraffic?.sent ?: it.sentBytes,
                    receivedBytes = local?.getOrNull(3) ?: remoteTraffic?.received ?: it.receivedBytes,
                    onlineConnections = remoteTraffic?.online ?: it.onlineConnections,
                    serverVersion = remote.updates?.serverVersion ?: it.serverVersion,
                    clientUpdate = remote.updates?.client ?: it.clientUpdate,
                )
            }
        } finally {
            leaseRefreshGate.finish()
        }
    }

    private fun confirmSessionLease(
        spec: TunnelLaunchSpec,
        remote: SessionTelemetry,
        nowEpochSeconds: Long,
        renewed: Boolean,
    ) {
        val leaseChanged = remote.expiresAtEpochSeconds != spec.sessionExpiresAtEpochSeconds
        val confirmedAccessExpiry = remote.accessExpiresAtEpochSeconds ?: spec.accessExpiresAtEpochSeconds
        val updated = spec.withSessionExpiry(remote.expiresAtEpochSeconds, confirmedAccessExpiry)
        launchSpec = updated
        if (renewed || leaseChanged) {
            renewAtEpochSeconds = AndroidLeaseRenewalSchedule.nextRenewalEpochSeconds(
                nowEpochSeconds,
                remote.expiresAtEpochSeconds,
            )
        }
        if (
            remote.expiresAtEpochSeconds == spec.sessionExpiresAtEpochSeconds &&
            confirmedAccessExpiry == spec.accessExpiresAtEpochSeconds
        ) {
            return
        }
        runCatching {
            val saved = credentialStore.load() ?: return@runCatching
            if (saved.launchSpec()?.sessionToken != spec.sessionToken) return@runCatching
            credentialStore.save(
                DurableTunnelCredentials(
                    profile = saved.profile,
                    sessionManifest = saved.sessionManifest,
                    accessExpiresAtEpochSeconds = confirmedAccessExpiry
                        ?: saved.accessExpiresAtEpochSeconds,
                    sessionExpiresAtEpochSeconds = remote.expiresAtEpochSeconds,
                ),
            )
        }.onFailure { error ->
            AndroidConnectionDiagnostics.record(
                stage = "lease_persistence",
                outcome = DiagnosticOutcome.Failed,
                phase = KetTunnelRuntime.snapshot().phase,
                protocol = spec.preferredProtocol,
                transport = activeTransportId,
                code = "lease_persistence_failed",
                message = error.message,
            )
        }
    }

    private fun requestStop(failure: String?) {
        if (!lifecycle.tryStop()) return
        KetTunnelRuntime.update {
            it.copy(
                phase = TunnelPhase.Stopping,
                message = if (failure == null) {
                    "Stopping protected route..."
                } else {
                    "Closing the failed protected route..."
                },
            )
        }
        worker.execute { stopTunnel(failure) }
    }

    private fun stopTunnel(failure: String?) {
        healthTask?.cancel(true)
        telemetryTask?.cancel(true)
        networkChangeTask?.cancel(true)
        underlyingNetworks.clearConnection()
        reconnecting.set(false)
        val localCleanupSucceeded = cleanupLocal()
        val spec = launchSpec
        launchSpec = null
        val release = spec?.let {
            runCatching { KetControlApi.release(it.controlEndpoint, it.sessionToken, it.controlNode) }
        }
        val releaseConclusive = TunnelCredentialCleanupPolicy.releaseIsConclusive(
            release?.exceptionOrNull(),
        )
        val accessExpired = spec?.accessExpiresAtEpochSeconds
            ?.let { it <= System.currentTimeMillis() / 1_000 } == true
        val credentialCleanup = runCatching {
            TunnelCredentialCleanupPolicy.apply(
                credentialStore,
                TunnelCredentialCleanupPolicy.afterRelease(
                    sessionExists = spec != null,
                    releaseConclusive = releaseConclusive,
                    accessExpired = accessExpired,
                ),
            )
        }
        val cleanupSucceeded = localCleanupSucceeded && releaseConclusive && credentialCleanup.isSuccess
        AndroidConnectionDiagnostics.record(
            stage = "disconnect_cleanup",
            outcome = if (cleanupSucceeded) DiagnosticOutcome.Succeeded else DiagnosticOutcome.Failed,
            phase = if (failure == null) TunnelPhase.Disconnected else TunnelPhase.Failed,
            protocol = spec?.preferredProtocol,
            code = if (cleanupSucceeded) null else "cleanup_incomplete",
            message = release?.exceptionOrNull()?.message ?: credentialCleanup.exceptionOrNull()?.message,
        )
        lifecycle.stageTerminal(
            if (failure == null) {
                TunnelSnapshot()
            } else {
                TunnelSnapshot(
                    phase = TunnelPhase.Failed,
                    node = spec?.node ?: KetTunnelRuntime.snapshot().node,
                    message = failure,
                    serverVersion = KetTunnelRuntime.snapshot().serverVersion,
                    clientUpdate = KetTunnelRuntime.snapshot().clientUpdate,
                )
            },
        )
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    @Synchronized
    private fun stopTransportAndBridge(): Boolean {
        var succeeded = true
        if (bridgeActive) runCatching { bridge.stop() }.onFailure { succeeded = false }
        bridgeActive = false
        runCatching { engine?.close() }.onFailure { succeeded = false }
        engine = null
        activeTransportId = null
        bridgeConfig?.let { config ->
            if (config.exists() && !config.delete()) succeeded = false
        }
        bridgeConfig = null
        return succeeded
    }

    @Synchronized
    private fun cleanupLocal(): Boolean {
        var succeeded = stopTransportAndBridge()
        runCatching { vpnRoute.close() }.onFailure { succeeded = false }
        transportAddresses = emptyMap()
        return succeeded
    }

    private fun resolveTransportAddresses(transports: List<AndroidTransport>): Map<String, InetAddress> =
        transports.mapNotNull { transport ->
            runCatching { transport.id to resolveTransportAddress(transport.endpoint) }.getOrNull()
        }.toMap()

    private fun writeBridgeConfig(document: String): File {
        return File.createTempFile("tun-to-socks-", ".yml", cacheDir).also {
            it.writeText(document)
            Os.chmod(it.absolutePath, 0x180) // 0600
        }
    }

    /** Restricts the interface to the apps the user chose (Settings, "Apps that use Ket"). */
    private fun applySplitTunnel(builder: Builder) {
        val applied = SplitTunnelRules.apply(
            policy = SplitTunnelStore(this).load(),
            ownPackage = packageName,
            isInstalled = { InstalledApps.isInstalled(this, it) },
            rules = object : VpnAppRules {
                // An app removed after the installed check is left out: it can
                // no longer send traffic either way.
                override fun allow(packageName: String) {
                    runCatching { builder.addAllowedApplication(packageName) }
                }

                override fun disallow(packageName: String) {
                    runCatching { builder.addDisallowedApplication(packageName) }
                }
            },
        )
        AndroidConnectionDiagnostics.record(
            stage = "split_tunnel",
            outcome = DiagnosticOutcome.Observed,
            phase = TunnelPhase.Connecting,
            code = applied.mode.wireName,
            message = applied.fallbackReason ?: "${applied.appCount} app(s)",
        )
    }

    private fun addVpnRoutes(builder: Builder, bypassAddresses: Collection<InetAddress>) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            builder.addRoute("0.0.0.0", 0)
            builder.addRoute("::", 0)
            bypassAddresses.forEach { address ->
                builder.excludeRoute(IpPrefix(address, if (address is Inet4Address) 32 else 128))
            }
            return
        }

        val ipv4 = bypassAddresses.filterIsInstance<Inet4Address>()
        if (ipv4.isEmpty()) {
            builder.addRoute("0.0.0.0", 0)
        } else {
            routesExcluding(ipv4).forEach {
                builder.addRoute(requireNotNull(it.address.hostAddress), it.prefixLength)
            }
        }
        val ipv6 = bypassAddresses.filterNot { it is Inet4Address }
        if (ipv6.isEmpty()) {
            builder.addRoute("::", 0)
        } else {
            routesExcluding(ipv6).forEach {
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
            .setSmallIcon(com.ket.android.R.drawable.ket_notification)
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
        const val EXTRA_APP_STARTED = "com.ket.android.extra.APP_STARTED"
        private const val NOTIFICATION_CHANNEL = "ket-vpn"
        private const val NOTIFICATION_ID = 3712
        private const val TUN_MTU = 1400
        private const val TUN_IPV4 = "198.18.0.1"
        private const val TUN_IPV6 = "fc00::1"
        private const val RECONNECT_RETRY_MILLIS = 3_000L
        private const val NETWORK_CHANGE_DEBOUNCE_MILLIS = 1_500L
        private const val BRIDGE_SETTLE_MILLIS = 250L
    }
}

private data class SelectedTransport(
    val engine: AndroidTransportEngine,
    val started: AndroidEngineStarted,
)

internal data class RoutePrefix(val address: InetAddress, val prefixLength: Int)

internal fun routesExcluding(address: InetAddress): List<RoutePrefix> = routesExcluding(listOf(address))

internal fun routesExcluding(addresses: Collection<InetAddress>): List<RoutePrefix> {
    require(addresses.isNotEmpty()) { "At least one excluded address is required" }
    val excluded = addresses.distinct().map(InetAddress::getAddress)
    val addressSize = excluded.first().size
    require(excluded.all { it.size == addressSize }) { "Excluded addresses must use one IP family" }
    val routes = mutableListOf<RoutePrefix>()

    fun visit(prefix: ByteArray, prefixLength: Int, matches: List<ByteArray>) {
        if (prefixLength == addressSize * 8) return
        val byteIndex = prefixLength / 8
        val bitMask = 1 shl (7 - prefixLength % 8)
        for (setBit in listOf(false, true)) {
            val child = prefix.copyOf()
            if (setBit) child[byteIndex] = (child[byteIndex].toInt() or bitMask).toByte()
            val childMatches = matches.filter { address ->
                ((address[byteIndex].toInt() and bitMask) != 0) == setBit
            }
            if (childMatches.isEmpty()) {
                routes += RoutePrefix(InetAddress.getByAddress(child), prefixLength + 1)
            } else {
                visit(child, prefixLength + 1, childMatches)
            }
        }
    }

    visit(ByteArray(addressSize), 0, excluded)
    return routes
}
