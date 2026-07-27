package com.ket.android

import java.net.InetAddress
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test

class AndroidOpenVpnManagementTest {
    @Test
    fun `validates a complete dual-stack tunnel request`() {
        val config = AndroidOpenVpnManagement.MutableTunConfig()
        config.applyIfConfig("10.8.0.2 255.255.255.0 1400 subnet")
        config.applyIfConfig6("fd42::2/64 1380")
        config.addDns("1.1.1.1")
        config.addDns("2606:4700:4700::1111")

        val request = config.validated()
        assertEquals(InetAddress.getByName("10.8.0.2"), request.ipv4Address)
        assertEquals(24, request.ipv4PrefixLength)
        assertEquals(InetAddress.getByName("fd42::2"), request.ipv6Address)
        assertEquals(64, request.ipv6PrefixLength)
        assertEquals(1_380, request.mtu)
        assertEquals(
            listOf(InetAddress.getByName("1.1.1.1"), InetAddress.getByName("2606:4700:4700::1111")),
            request.dnsServers,
        )
    }

    @Test
    fun `requires private IPv4 contiguous masks bounded MTU and numeric DNS`() {
        val invalidInterfaces = listOf(
            "203.0.113.2 255.255.255.0 1400 subnet",
            "10.8.0.2 255.0.255.0 1400 subnet",
            "10.8.0.2 255.255.255.0 999 subnet",
            "10.8.0.2 255.255.255.0 1501 subnet",
            "10.8.0.2 255.255.255.0 1400 net30",
            "hostname 255.255.255.0 1400 subnet",
        )
        invalidInterfaces.forEach { value ->
            assertThrows(IllegalArgumentException::class.java) {
                AndroidOpenVpnManagement.MutableTunConfig().applyIfConfig(value)
            }
        }
        for (dns in listOf("localhost", "127.0.0.1", "0.0.0.0", "224.0.0.1", "::1")) {
            assertThrows(IllegalArgumentException::class.java) {
                AndroidOpenVpnManagement.MutableTunConfig().addDns(dns)
            }
        }
    }

    @Test
    fun `bounds DNS count and refuses incomplete tunnel requests`() {
        val incomplete = AndroidOpenVpnManagement.MutableTunConfig()
        assertThrows(IllegalArgumentException::class.java) { incomplete.validated() }

        incomplete.applyIfConfig("10.8.0.2 255.255.255.0 1400 subnet")
        assertThrows(IllegalArgumentException::class.java) { incomplete.validated() }

        listOf("1.1.1.1", "8.8.8.8", "9.9.9.9", "208.67.222.222").forEach(incomplete::addDns)
        assertThrows(IllegalArgumentException::class.java) { incomplete.addDns("4.2.2.2") }
        val request = incomplete.validated()
        assertNull(request.ipv6Address)
        assertNull(request.ipv6PrefixLength)
        assertEquals(4, request.dnsServers.size)
    }
}
