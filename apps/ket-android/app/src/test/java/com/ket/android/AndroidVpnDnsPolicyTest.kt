package com.ket.android

import java.net.InetAddress
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidVpnDnsPolicyTest {
    @Test
    fun `selects bridge-local mapped DNS`() {
        assertEquals(InetAddress.getByName("198.18.0.2"), AndroidVpnDnsPolicy.server())
    }

    @Test
    fun `tun bridge maps DNS names into socks domain requests`() {
        val config = EngineConfig.tunToSocks(10808)

        assertTrue(config.contains("address: '127.0.0.1'"))
        assertTrue(config.contains("port: 10808"))
        assertTrue(config.contains("udp: 'udp'"))
        assertTrue(config.contains("mapdns:"))
        assertTrue(config.contains("address: 198.18.0.2"))
        assertTrue(config.contains("network: 240.0.0.0"))
        assertTrue(config.contains("netmask: 240.0.0.0"))
        assertTrue(config.contains("cache-size: 10000"))
    }
}
