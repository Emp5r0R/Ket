package com.ket.android.share

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

    fun candidates(interfaces: List<LocalInterface>): List<ShareNetwork> = interfaces
        .filter { it.up && !it.loopback }
        .flatMap { local ->
            val kind = kindOf(local.name) ?: return@flatMap emptyList()
            local.addresses
                .filterIsInstance<Inet4Address>()
                .filter(::isPrivate)
                .map { ShareNetwork(local.name, requireNotNull(it.hostAddress), kind) }
        }
        .distinctBy { it.address }
        .sortedBy { it.kind.ordinal }

    /** The networks the phone is on now. */
    fun current(): List<ShareNetwork> = candidates(
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
    )

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
