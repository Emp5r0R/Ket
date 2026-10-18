package com.ket.android.share

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import com.ket.android.KetTunnelRuntime
import com.ket.android.TunnelPhase
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit

/**
 * The app's one share. It lives in the app process, which Ket's VPN foreground service keeps
 * running, so sharing goes on while the app is in the background.
 *
 * Connections leave only through Ket's own VPN network: the app follows its default network,
 * which is Ket's VPN while it is up because Ket itself always uses its VPN, and passes that
 * network's handle to the proxy. Any other network (Wi-Fi, mobile data) is never passed, so with
 * the VPN down nothing a nearby device sends leaves the phone. Sharing stops when Ket disconnects.
 */
object KetShare {
    private const val TICK_SECONDS = 2L

    val controller = ShareController(NativeShareEngine, ShareNetworks::current)

    private val scheduler = Executors.newSingleThreadScheduledExecutor { runnable ->
        Thread(runnable, "ket-share-watch").apply { isDaemon = true }
    }
    private val lock = Any()
    private var ticking: ScheduledFuture<*>? = null
    private var callback: ConnectivityManager.NetworkCallback? = null
    private var tunnelSubscription: AutoCloseable? = null
    private var connectivity: ConnectivityManager? = null

    @Volatile private var defaultVpn = 0L

    @Volatile private var ketConnected = false

    fun start(context: Context, address: String): Boolean {
        watch(context.applicationContext)
        val started = controller.start(address)
        if (!started) unwatch()
        return started
    }

    fun stop(notice: ShareNotice? = null) {
        controller.stop(notice)
        unwatch()
    }

    private fun watch(context: Context) = synchronized(lock) {
        if (callback != null) return@synchronized
        val manager = context.getSystemService(ConnectivityManager::class.java) ?: return@synchronized
        connectivity = manager
        val networkCallback = object : ConnectivityManager.NetworkCallback() {
            override fun onCapabilitiesChanged(network: Network, capabilities: NetworkCapabilities) {
                defaultVpn = if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) {
                    network.networkHandle
                } else {
                    0L
                }
                passNetwork()
            }

            override fun onLost(network: Network) {
                if (network.networkHandle == defaultVpn) defaultVpn = 0L
                passNetwork()
            }
        }
        runCatching { manager.registerDefaultNetworkCallback(networkCallback) }
            .onSuccess { callback = networkCallback }
        tunnelSubscription = KetTunnelRuntime.subscribe { snapshot ->
            ketConnected = snapshot.phase == TunnelPhase.Connected
            when (snapshot.phase) {
                TunnelPhase.Disconnected, TunnelPhase.Stopping, TunnelPhase.Failed ->
                    if (controller.isSharing) scheduler.execute { stop(ShareNotice.KetDisconnected) }
                else -> passNetwork()
            }
        }
        ticking = scheduler.scheduleWithFixedDelay(
            {
                runCatching { controller.tick() }
                if (!controller.isSharing) unwatch()
            },
            TICK_SECONDS,
            TICK_SECONDS,
            TimeUnit.SECONDS,
        )
    }

    /** Only Ket's VPN, and only while Ket says it is connected; otherwise none. */
    private fun passNetwork() {
        controller.vpnNetworkChanged(if (ketConnected) defaultVpn else 0L)
    }

    private fun unwatch() = synchronized(lock) {
        ticking?.cancel(false)
        ticking = null
        callback?.let { registered -> runCatching { connectivity?.unregisterNetworkCallback(registered) } }
        callback = null
        tunnelSubscription?.close()
        tunnelSubscription = null
        defaultVpn = 0L
        controller.vpnNetworkChanged(0L)
    }
}
