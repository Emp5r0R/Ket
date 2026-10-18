package com.ket.android.share

import java.util.concurrent.CopyOnWriteArraySet

/** The running share, as nearby devices need it. The password is a secret: [toString] leaves it out. */
data class ActiveShare(val network: ShareNetwork, val port: Int, val username: String, val password: String) {
    /** `http://ket:PASSWORD@ADDRESS:PORT`, which proxy apps can import. */
    val proxyUrl: String get() = "http://$username:$password@${network.address}:$port"

    override fun toString(): String =
        "ActiveShare(network=$network, port=$port, username=$username, password=[REDACTED])"
}

/** Why sharing did not start, or why it stopped on its own. */
enum class ShareNotice(val message: String) {
    NetworkUnavailable("This phone is no longer on that network. Choose another one."),
    PortUnavailable("Ket could not open a port on that network. Try again, or choose another network."),
    Failed("Sharing could not start. Try again."),
    NetworkGone("Sharing stopped because this phone left the network it was sharing on."),
    KetDisconnected("Sharing stopped because Ket disconnected."),
}

data class ShareState(
    val active: ActiveShare? = null,
    val devices: Int = 0,
    val refusedSignIns: Long = 0,
    /** A VPN network is there to carry the devices' traffic right now. */
    val tunnel: Boolean = false,
    val notice: ShareNotice? = null,
)

/**
 * Starts and stops the share and keeps its state: the device count, whether the VPN can carry
 * traffic, and stopping on its own when the phone leaves the network it shares on. [tick] is
 * called every couple of seconds while sharing; [vpnNetworkChanged] whenever Ket's VPN network
 * appears or goes.
 */
class ShareController(
    private val engine: ShareEngine,
    private val currentNetworks: () -> List<ShareNetwork>,
) {
    private val lock = Any()
    private val listeners = CopyOnWriteArraySet<(ShareState) -> Unit>()
    private var state = ShareState()
    private var handle: Long? = null
    private var vpnNetwork = 0L

    fun state(): ShareState = synchronized(lock) { state }

    fun subscribe(listener: (ShareState) -> Unit): AutoCloseable {
        listeners += listener
        listener(state())
        return AutoCloseable { listeners -= listener }
    }

    /** Starts sharing on [address] with a new password, replacing any share already running. */
    fun start(address: String): Boolean {
        stop(notice = null)
        val network = currentNetworks().firstOrNull { it.address == address }
        if (network == null) {
            publish(ShareState(notice = ShareNotice.NetworkUnavailable))
            return false
        }
        return when (val started = engine.start(address)) {
            is ShareStart.Refused -> {
                publish(ShareState(notice = refusal(started.code)))
                false
            }
            is ShareStart.Started -> synchronized(lock) {
                val share = started.share
                handle = share.handle
                engine.setNetwork(share.handle, vpnNetwork)
                state = ShareState(
                    active = ActiveShare(network, share.port, share.username, share.password),
                    tunnel = vpnNetwork != 0L,
                )
                true
            }.also { notifyListeners() }
        }
    }

    /** Stops sharing and closes every device's connections. */
    fun stop(notice: ShareNotice? = null) {
        val running = synchronized(lock) { handle.also { handle = null } } ?: run {
            if (notice != null) publish(state().copy(notice = notice))
            return
        }
        engine.stop(running)
        publish(ShareState(notice = notice))
    }

    /** Ket's VPN network handle, or 0 while it is down; connections wait for it. */
    fun vpnNetworkChanged(network: Long) {
        val running = synchronized(lock) {
            vpnNetwork = network
            handle?.also { state = state.copy(tunnel = network != 0L) }
        } ?: return
        engine.setNetwork(running, network)
        notifyListeners()
    }

    fun tick() {
        val (running, address) = synchronized(lock) { handle to state.active?.network?.address }
        if (running == null || address == null) return
        if (currentNetworks().none { it.address == address }) {
            stop(ShareNotice.NetworkGone)
            return
        }
        val counts = engine.counts(running) ?: return stop(ShareNotice.Failed)
        synchronized(lock) {
            if (handle != running) return
            state = state.copy(devices = counts.devices, refusedSignIns = counts.refusedSignIns)
        }
        notifyListeners()
    }

    val isSharing: Boolean get() = synchronized(lock) { handle != null }

    private fun refusal(code: String): ShareNotice = when (code) {
        "network_unavailable" -> ShareNotice.NetworkUnavailable
        "port_unavailable" -> ShareNotice.PortUnavailable
        else -> ShareNotice.Failed
    }

    private fun publish(next: ShareState) {
        synchronized(lock) { state = next }
        notifyListeners()
    }

    private fun notifyListeners() {
        val current = state()
        listeners.forEach { it(current) }
    }
}
