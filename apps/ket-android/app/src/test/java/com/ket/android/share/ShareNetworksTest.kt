package com.ket.android.share

import java.net.InetAddress
import org.junit.Assert.assertEquals
import org.junit.Test

class ShareNetworksTest {
    private fun local(name: String, vararg addresses: String, up: Boolean = true, loopback: Boolean = false) =
        LocalInterface(name, up, loopback, addresses.map(InetAddress::getByName))

    @Test
    fun `hotspot and wifi and tethering on private addresses are offered, hotspot first`() {
        val networks = ShareNetworks.candidates(
            listOf(
                local("wlan0", "192.168.1.40", "fe80::1"),
                local("swlan0", "192.168.43.1"),
                local("rndis0", "10.42.0.1"),
                local("eth0", "172.20.1.5"),
            ),
        )
        assertEquals(
            listOf(
                ShareNetwork("swlan0", "192.168.43.1", ShareNetworkKind.Hotspot),
                ShareNetwork("wlan0", "192.168.1.40", ShareNetworkKind.WiFi),
                ShareNetwork("rndis0", "10.42.0.1", ShareNetworkKind.Usb),
                ShareNetwork("eth0", "172.20.1.5", ShareNetworkKind.Ethernet),
            ),
            networks,
        )
        assertEquals("Hotspot · 192.168.43.1", networks.first().label)
    }

    @Test
    fun `mobile data, the vpn, loopback, public addresses, and interfaces that are down are never offered`() {
        val networks = ShareNetworks.candidates(
            listOf(
                local("rmnet_data0", "10.64.12.9"),
                local("ccmni1", "10.1.1.1"),
                local("tun0", "10.0.0.2"),
                local("lo", "127.0.0.1", loopback = true),
                local("wlan0", "203.0.113.7"),
                local("wlan1", "192.168.5.1", up = false),
                local("wlan_p2p", "192.168.49.1"),
                local("wlan2", "100.64.0.1"),
            ),
        )
        assertEquals(emptyList<ShareNetwork>(), networks)
    }

    @Test
    fun `private means rfc 1918`() {
        listOf("10.0.0.1", "172.16.0.1", "172.31.255.254", "192.168.0.1").forEach {
            assert(ShareNetworks.isPrivate(InetAddress.getByName(it) as java.net.Inet4Address)) { it }
        }
        listOf("172.15.0.1", "172.32.0.1", "100.64.0.1", "169.254.0.1", "8.8.8.8").forEach {
            assert(!ShareNetworks.isPrivate(InetAddress.getByName(it) as java.net.Inet4Address)) { it }
        }
    }
}
