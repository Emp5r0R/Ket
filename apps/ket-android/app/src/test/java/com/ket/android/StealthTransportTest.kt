package com.ket.android

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Test

class StealthTransportTest {
    @Test
    fun `strict stealth profile is preferred and rendered as XHTTP TLS`() {
        val transport = AndroidTransportSelector.parse(JSONArray().put(stealthTransport())).first()
            as StealthTransport
        val config = JSONObject(EngineConfig.xray(transport, "203.0.113.9", 10808))
        val outbound = config.getJSONArray("outbounds").getJSONObject(0)
        val user = outbound.getJSONObject("settings")
            .getJSONArray("vnext").getJSONObject(0)
            .getJSONArray("users").getJSONObject(0)
        val stream = outbound.getJSONObject("streamSettings")

        assertEquals("HTTPS Stealth", transport.displayName)
        assertEquals("ket-stealth", outbound.getString("tag"))
        assertEquals("203.0.113.9", outbound.getJSONObject("settings").getJSONArray("vnext").getJSONObject(0).getString("address"))
        assertFalse(user.has("flow"))
        assertEquals("xhttp", stream.getString("network"))
        assertEquals("tls", stream.getString("security"))
        assertEquals("stealth.example.test", stream.getJSONObject("tlsSettings").getString("serverName"))
        assertEquals("packet-up", stream.getJSONObject("xhttpSettings").getString("mode"))
        assertFalse(stream.getJSONObject("tlsSettings").has("allowInsecure"))
        assertFalse(transport.toString().contains("550e8400"))
        assertFalse(transport.toString().contains("a1b2c3d4"))
    }

    @Test
    fun `unknown secrets and unsafe paths are rejected`() {
        val secret = stealthTransport()
        secret.getJSONObject("credential").getJSONObject("secrets").put("token", "unexpected")
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(JSONArray().put(secret))
        }

        val path = stealthTransport()
        path.getJSONObject("options").put("path", "/short")
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(JSONArray().put(path))
        }
    }

    private fun stealthTransport(): JSONObject = JSONObject()
        .put("id", "https-stealth-primary")
        .put("protocol", "stealth")
        .put("endpoint", "stealth.example.test")
        .put("port", 443)
        .put("network", "tcp")
        .put("priority", 1)
        .put("tls_server_name", "stealth.example.test")
        .put(
            "options",
            JSONObject()
                .put("encryption", "none")
                .put("fingerprint", "chrome")
                .put("mode", "packet-up")
                .put("path", "/a1b2c3d4e5f6g7h8")
                .put("security", "tls")
                .put("transport", "xhttp"),
        )
        .put(
            "credential",
            JSONObject()
                .put("auth", "550e8400-e29b-41d4-a716-446655440000")
                .put("secrets", JSONObject()),
        )
}
