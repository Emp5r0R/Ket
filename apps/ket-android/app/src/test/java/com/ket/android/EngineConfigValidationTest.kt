package com.ket.android

import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class EngineConfigValidationTest {
    @Test
    fun `TUN bridge config is deterministic bounded and DNS-policy aligned`() {
        val config = EngineConfig.tunToSocks(10_808)
        assertTrue(config.endsWith("\n"))
        assertTrue(config.contains("address: '127.0.0.1'"))
        assertTrue(config.contains("port: 10808"))
        assertTrue(config.contains("address: ${AndroidVpnDnsPolicy.SERVER}"))
        assertTrue(config.contains("network: ${AndroidVpnDnsPolicy.NETWORK}"))
        assertTrue(config.contains("netmask: ${AndroidVpnDnsPolicy.NETMASK}"))
        assertFalse(config.contains("log-level: debug"))
        for (port in listOf(Int.MIN_VALUE, -1, 0, 65_536, Int.MAX_VALUE)) {
            assertThrows(IllegalArgumentException::class.java) { EngineConfig.tunToSocks(port) }
        }
    }

    @Test
    fun `unsupported Xray implementations and invalid input are rejected before launch`() {
        val unsupported = object : AndroidXrayTransport {
            override val id = "unsupported"
            override val endpoint = "vpn.example.test"
            override val port = 443
            override val priority = 1
            override val displayName = "Unsupported"
            override val protocol = KetProtocol.Reality
            override val tlsServerName = "www.example.test"
            override val userId = "550e8400-e29b-41d4-a716-446655440000"
            override val fingerprint = "chrome"
        }
        assertThrows(IllegalArgumentException::class.java) {
            EngineConfig.xray(unsupported, "203.0.113.9", 10_808)
        }
        assertThrows(IllegalArgumentException::class.java) {
            EngineConfig.xray(unsupported, "", 10_808)
        }
        assertThrows(IllegalArgumentException::class.java) {
            EngineConfig.xray(unsupported, "203.0.113.9", 0)
        }
    }
}
