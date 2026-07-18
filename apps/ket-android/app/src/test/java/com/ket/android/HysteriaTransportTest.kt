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
    fun `enrollment parser requires an implemented Android transport and redacts the token`() {
        val body = JSONObject()
            .put("session_token", "control-token-value")
            .put(
                "node",
                JSONObject()
                    .put("display_name", "Test node")
                    .put("location", JSONObject().put("country_name", "Testland")),
            )
            .put("transports", JSONArray().put(validTransport("salamander")))
            .toString()

        val result = KetControlApi.parseEnrollment(body)

        assertEquals("Test node", result.node)
        assertEquals("Testland", result.country)
        assertEquals("Hysteria 2", result.transports.single().displayName)
        assertFalse(result.toString().contains("control-token-value"))
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
