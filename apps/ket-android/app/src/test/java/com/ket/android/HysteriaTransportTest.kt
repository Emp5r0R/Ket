package com.ket.android

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class HysteriaTransportTest {
    @Test
    fun `selects and renders a strict gecko transport without leaking secrets`() {
        val transport = HysteriaTransport.select(
            JSONArray().put(validTransport("gecko")),
        )

        val config = JSONObject(
            EngineConfig.hysteria(transport, "203.0.113.8", "ket_fd_test", 13820),
        )

        assertEquals("203.0.113.8:443", config.getString("server"))
        assertEquals("vpn.example.test", config.getJSONObject("tls").getString("sni"))
        assertFalse(config.getJSONObject("tls").has("insecure"))
        assertEquals("@ket_fd_test", config.getJSONObject("quic").getJSONObject("sockopts").getString("fdControlUnixSocket"))
        assertEquals("gecko", config.getJSONObject("obfs").getString("type"))
        assertEquals(512, config.getJSONObject("obfs").getJSONObject("gecko").getInt("minPacketSize"))
        assertFalse(transport.toString().contains("lease-secret-value"))
        assertFalse(transport.toString().contains("obfs-secret-value"))
    }

    @Test
    fun `rejects unknown options instead of accepting a downgrade-shaped profile`() {
        val profile = validTransport("none")
        profile.getJSONObject("options").put("insecure", "true")

        val error = assertThrows(IllegalArgumentException::class.java) {
            HysteriaTransport.select(JSONArray().put(profile))
        }

        assertTrue(error.message.orEmpty().contains("Unsupported transport option"))
    }

    @Test
    fun `rejects an obfuscation password when obfuscation is disabled`() {
        val profile = validTransport("none")
        profile.getJSONObject("credential").getJSONObject("secrets").put("obfs_password", "unexpected")

        assertThrows(IllegalArgumentException::class.java) {
            HysteriaTransport.select(JSONArray().put(profile))
        }
    }

    @Test
    fun `selector rejects excessive malformed and duplicate transport identities`() {
        val excessive = JSONArray().apply {
            repeat(33) { put(validTransport("none").put("id", "hy2-$it")) }
        }
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(excessive)
        }

        val malformed = JSONArray().put(validTransport("none").put("id", "../hy2"))
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(malformed)
        }

        val duplicates = JSONArray()
            .put(validTransport("none"))
            .put(validTransport("none"))
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(duplicates)
        }
    }

    @Test
    fun `transport credentials are bounded before engine configuration`() {
        val profile = validTransport("none")
        profile.getJSONObject("credential").put("auth", "x".repeat(4_097))

        assertThrows(IllegalArgumentException::class.java) {
            HysteriaTransport.select(JSONArray().put(profile))
        }
    }

    @Test
    fun `enrollment parser requires an implemented Android transport and redacts the token`() {
        val token = testSessionToken('H')
        val body = JSONObject()
            .put("session_token", token)
            .put("session_expires_at_epoch_seconds", 4_000_000_000)
            .put("node", testNodeJson())
            .put("transports", JSONArray().put(validTransport("salamander")))
            .toString()

        val result = KetControlApi.parseEnrollment(body)

        assertEquals("Test node", result.node.displayName)
        assertEquals("Testland", result.node.location.countryName)
        assertEquals("Hysteria 2", result.transports.single().displayName)
        assertFalse(result.toString().contains(token))
    }

    private fun validTransport(obfs: String): JSONObject {
        val options = JSONObject().put("obfs", obfs)
        if (obfs == "gecko") {
            options.put("gecko_min_packet_size", "512")
            options.put("gecko_max_packet_size", "1200")
        }
        val secrets = JSONObject()
        if (obfs != "none") secrets.put("obfs_password", "obfs-secret-value")
        return JSONObject()
            .put("id", "hy2-primary")
            .put("protocol", "hysteria2")
            .put("endpoint", "vpn.example.test")
            .put("port", 443)
            .put("network", "udp")
            .put("priority", 10)
            .put("tls_server_name", "vpn.example.test")
            .put("options", options)
            .put(
                "credential",
                JSONObject()
                    .put("auth", "lease-secret-value")
                    .put("secrets", secrets),
            )
    }
}
