package com.ket.android

import com.ket.android.connection.BypassEntries
import com.ket.android.connection.BypassCheck
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

    @Test
    fun `multiple endpoints stay outside the guarded route`() {
        val excluded = listOf("203.0.113.8", "203.0.113.10").map(InetAddress::getByName)
        val routes = routesExcluding(excluded)

        excluded.forEach { address -> assertFalse(routes.any { it.contains(address) }) }
        listOf("0.0.0.0", "203.0.113.7", "203.0.113.9", "203.0.113.11", "255.255.255.255")
            .map(InetAddress::getByName)
            .forEach { sample -> assertTrue("$sample was not routed", routes.any { it.contains(sample) }) }
    }

    @Test
    fun `adjacent endpoint exclusions collapse their common prefix`() {
        val routes = routesExcluding(
            listOf("203.0.113.8", "203.0.113.9").map(InetAddress::getByName),
        )

        assertEquals(31, routes.size)
    }

    @Test
    fun `bypassed networks stay outside the route alongside the servers`() {
        val excluded = listOf("192.168.1.0/24", "203.0.113.9").map {
            (BypassEntries.check(it) as BypassCheck.Accepted).prefix
        }
        val routes = routesExcludingPrefixes(excluded)

        listOf("192.168.1.0", "192.168.1.77", "192.168.1.255", "203.0.113.9")
            .map(InetAddress::getByName)
            .forEach { address -> assertFalse("$address was routed", routes.any { it.contains(address) }) }
        listOf("0.0.0.0", "192.168.0.255", "192.168.2.0", "203.0.113.8", "255.255.255.255")
            .map(InetAddress::getByName)
            .forEach { sample -> assertTrue("$sample was not routed", routes.any { it.contains(sample) }) }
        // One route beside each level of the two paths: four shared levels, then 19 more to
        // the /24 and 27 more to the host after they part at the fifth.
        assertEquals(4 + 19 + 27, routes.size)
    }

    @Test
    fun `an ipv6 complement stays ipv6 where it looks like ipv4`() {
        val routes = routesExcludingPrefixes(listOf((BypassEntries.check("2001:db8::/32") as BypassCheck.Accepted).prefix))
        assertEquals(32, routes.size)
        assertTrue(routes.all { it.address.address.size == 16 })
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
