package com.ket.android

import java.net.Inet4Address
import java.net.InetAddress

/** Selects explicit dual-stack resolvers that cannot overlap a transport route exclusion. */
internal object AndroidVpnDnsPolicy {
    private val candidates = listOf(
        "1.1.1.1",
        "1.0.0.1",
        "2606:4700:4700::1111",
        "2606:4700:4700::1001",
    ).map(InetAddress::getByName)

    fun serversFor(bypassAddresses: Collection<InetAddress>): List<InetAddress> {
        val bypasses = bypassAddresses.toSet()
        val selected = candidates.filterNot(bypasses::contains)
        require(selected.any { it is Inet4Address }) {
            "Every configured IPv4 VPN DNS server overlaps a transport endpoint"
        }
        require(selected.any { it !is Inet4Address }) {
            "Every configured IPv6 VPN DNS server overlaps a transport endpoint"
        }
        return selected
    }
}
