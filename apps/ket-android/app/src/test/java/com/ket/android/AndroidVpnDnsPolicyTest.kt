package com.ket.android

import java.net.Inet4Address
import java.net.InetAddress
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidVpnDnsPolicyTest {
    @Test
    fun `selects redundant explicit resolvers for both address families`() {
        val selected = AndroidVpnDnsPolicy.serversFor(emptyList()).toSet()

        assertEquals(
            setOf(
                address("1.1.1.1"),
                address("1.0.0.1"),
                address("2606:4700:4700::1111"),
                address("2606:4700:4700::1001"),
            ),
            selected,
        )
    }

    @Test
    fun `removes resolver addresses excluded for a transport without losing either family`() {
        val bypasses = listOf(address("1.1.1.1"), address("2606:4700:4700::1111"))

        val selected = AndroidVpnDnsPolicy.serversFor(bypasses)

        assertFalse(selected.any(bypasses::contains))
        assertEquals(listOf(address("1.0.0.1"), address("2606:4700:4700::1001")), selected)
    }

    @Test
    fun `fails closed when transport exclusions consume either resolver family`() {
        val ipv4Error = assertThrows(IllegalArgumentException::class.java) {
            AndroidVpnDnsPolicy.serversFor(
                listOf(address("1.1.1.1"), address("1.0.0.1")),
            )
        }
        val ipv6Error = assertThrows(IllegalArgumentException::class.java) {
            AndroidVpnDnsPolicy.serversFor(
                listOf(
                    address("2606:4700:4700::1111"),
                    address("2606:4700:4700::1001"),
                ),
            )
        }

        assertTrue(ipv4Error.message.orEmpty().contains("IPv4 VPN DNS"))
        assertTrue(ipv6Error.message.orEmpty().contains("IPv6 VPN DNS"))
    }

    @Test
    fun `api 26 route complements keep every selected resolver inside the tunnel`() {
        val bypasses = listOf(address("203.0.113.9"), address("2001:db8::9"))
        val resolvers = AndroidVpnDnsPolicy.serversFor(bypasses)
        val ipv4Routes = routesExcluding(bypasses.filterIsInstance<Inet4Address>())
        val ipv6Routes = routesExcluding(bypasses.filterNot { it is Inet4Address })

        resolvers.forEach { resolver ->
            val routes = if (resolver is Inet4Address) ipv4Routes else ipv6Routes
            assertTrue("$resolver was not routed through the VPN", routes.any { it.contains(resolver) })
        }
    }

    @Test
    fun `tun bridge forwards udp DNS traffic through the selected socks engine`() {
        val config = EngineConfig.tunToSocks(10808)

        assertTrue(config.contains("address: '127.0.0.1'"))
        assertTrue(config.contains("port: 10808"))
        assertTrue(config.contains("udp: 'udp'"))
    }

    private fun address(value: String): InetAddress = InetAddress.getByName(value)

    private fun RoutePrefix.contains(candidate: InetAddress): Boolean {
        val network = address.address
        val value = candidate.address
        if (network.size != value.size) return false
        return (0 until prefixLength).all { bit ->
            val mask = 1 shl (7 - bit % 8)
            (network[bit / 8].toInt() and mask) == (value[bit / 8].toInt() and mask)
        }
    }
}
