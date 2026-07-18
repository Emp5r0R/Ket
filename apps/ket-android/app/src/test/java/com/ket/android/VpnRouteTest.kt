package com.ket.android

import java.net.InetAddress
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class VpnRouteTest {
    @Test
    fun `ipv4 complement covers every address except the transport server`() {
        assertComplement("203.0.113.9", listOf("0.0.0.0", "203.0.113.8", "203.0.113.10", "255.255.255.255"), 32)
    }

    @Test
    fun `ipv6 complement covers every address except the transport server`() {
        assertComplement("2001:db8::9", listOf("::", "2001:db8::8", "2001:db8::a", "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff"), 128)
    }

    private fun assertComplement(server: String, samples: List<String>, expectedPrefixes: Int) {
        val excluded = InetAddress.getByName(server)
        val routes = routesExcluding(excluded)
        assertEquals(expectedPrefixes, routes.size)
        assertFalse(routes.any { it.contains(excluded) })
        samples.map(InetAddress::getByName).forEach { sample ->
            assertTrue("$sample was not routed", routes.any { it.contains(sample) })
        }
    }

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
