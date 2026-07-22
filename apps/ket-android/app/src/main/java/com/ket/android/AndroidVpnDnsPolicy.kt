package com.ket.android

import java.net.InetAddress

/** Keeps DNS inside hev so SOCKS engines receive domains instead of client-selected IPv6 addresses. */
internal object AndroidVpnDnsPolicy {
    const val SERVER = "198.18.0.2"
    const val NETWORK = "240.0.0.0"
    const val NETMASK = "240.0.0.0"
    const val CACHE_SIZE = 10_000

    fun server(): InetAddress = InetAddress.getByName(SERVER)
}
