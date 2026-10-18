package com.ket.android.share

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.Build
import java.net.Inet4Address
import java.net.InetAddress
import java.net.NetworkInterface

/** What kind of local network nearby devices join to use the share. */
enum class ShareNetworkKind(val displayName: String) {
    Hotspot("Hotspot"),
    WiFi("Wi-Fi"),
    Usb("USB tethering"),
    Bluetooth("Bluetooth tethering"),
    Ethernet("Ethernet"),
}

/** An address of this phone on a network nearby devices can join. */
data class ShareNetwork(val interfaceName: String, val address: String, val kind: ShareNetworkKind) {
    val label: String get() = "${kind.displayName} · $address"
}

/** One interface as the system lists it. */
data class LocalInterface(
    val name: String,
    val up: Boolean,
    val loopback: Boolean,
    val addresses: List<InetAddress>,
)

/**
 * The networks the phone can share on: its hotspot, its Wi-Fi network, and USB, Bluetooth, or
 * Ethernet tethering, on private (RFC 1918) IPv4 addresses only. Interfaces are allowed by name
 * rather than refused by name, so mobile data (whose carrier addresses are often private too) and
 * the VPN itself are never offered.
 *
 * Many phones name their hotspot `wlan1` like a Wi-Fi connection, so when the system says which
 * interfaces carry the phone's own Wi-Fi connections, any other `wlan` interface is its hotspot.
 */
object ShareNetworks {
    private val kinds = listOf(
        "swlan" to ShareNetworkKind.Hotspot,
        "softap" to ShareNetworkKind.Hotspot,
        "ap" to ShareNetworkKind.Hotspot,
        "wlan" to ShareNetworkKind.WiFi,
        "rndis" to ShareNetworkKind.Usb,
        "usb" to ShareNetworkKind.Usb,
        "ncm" to ShareNetworkKind.Usb,
        "bt-pan" to ShareNetworkKind.Bluetooth,
        "eth" to ShareNetworkKind.Ethernet,
    )

    /**
     * [wifiClients] names the interfaces of the phone's Wi-Fi connections, or is null when the
     * system could not say.
     */
    fun candidates(interfaces: List<LocalInterface>, wifiClients: Set<String>? = null): List<ShareNetwork> = interfaces
        .filter { it.up && !it.loopback }
        .flatMap { local ->
            val named = kindOf(local.name) ?: return@flatMap emptyList()
            val kind = if (named == ShareNetworkKind.WiFi && wifiClients != null && local.name !in wifiClients) {
                ShareNetworkKind.Hotspot
            } else {
                named
            }
            local.addresses
                .filterIsInstance<Inet4Address>()
                .filter(::isPrivate)
                .map { ShareNetwork(local.name, requireNotNull(it.hostAddress), kind) }
        }
        .distinctBy { it.address }
        .sortedBy { it.kind.ordinal }

    /** The networks the phone is on now. */
    fun current(context: Context): List<ShareNetwork> = candidates(
        runCatching { NetworkInterface.getNetworkInterfaces()?.toList().orEmpty() }
            .getOrDefault(emptyList())
            .mapNotNull { network ->
                runCatching {
                    LocalInterface(
                        name = network.name,
                        up = network.isUp,
                        loopback = network.isLoopback,
                        addresses = network.inetAddresses.toList(),
                    )
                }.getOrNull()
            },
        wifiClientInterfaces(context),
    )

    /**
     * The interfaces of the phone's own Wi-Fi connections, or null when unknown. Android 15 and
     * later also list the hotspot as a Wi-Fi network, marked as a local network.
     */
    private fun wifiClientInterfaces(context: Context): Set<String>? = runCatching {
        val manager = context.getSystemService(ConnectivityManager::class.java) ?: return null
        @Suppress("DEPRECATION")
        manager.allNetworks.mapNotNullTo(mutableSetOf()) { network ->
            manager.getNetworkCapabilities(network)
                ?.takeIf { it.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) && !it.isLocalNetwork() }
                ?.let { manager.getLinkProperties(network)?.interfaceName }
        }
    }.getOrNull()

    private fun NetworkCapabilities.isLocalNetwork(): Boolean =
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.VANILLA_ICE_CREAM &&
            hasCapability(NetworkCapabilities.NET_CAPABILITY_LOCAL_NETWORK)

    private fun kindOf(name: String): ShareNetworkKind? =
        kinds.firstOrNull { (prefix, _) -> name.startsWith(prefix) && name.drop(prefix.length).all(Char::isDigit) }
            ?.second

    /** 10.0.0.0/8, 172.16.0.0/12, and 192.168.0.0/16. */
    fun isPrivate(address: Inet4Address): Boolean {
        val bytes = address.address.map { it.toInt() and 0xff }
        return bytes[0] == 10 ||
            (bytes[0] == 172 && bytes[1] in 16..31) ||
            (bytes[0] == 192 && bytes[1] == 168)
    }
}
