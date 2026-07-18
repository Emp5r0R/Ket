package com.ket.android

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class RealityTransportTest {
    @Test
    fun `strict reality profile is preferred and rendered for Xray`() {
        val transports = AndroidTransportSelector.parse(
            JSONArray()
                .put(hysteriaTransport())
                .put(realityTransport()),
        )
        val reality = transports.first() as RealityTransport
        val config = JSONObject(EngineConfig.xray(reality, "203.0.113.9", 10808))
        val outbound = config.getJSONArray("outbounds").getJSONObject(0)
        val stream = outbound.getJSONObject("streamSettings")
        val settings = stream.getJSONObject("realitySettings")

        assertEquals("VLESS + REALITY", reality.displayName)
        assertEquals("203.0.113.9", outbound.getJSONObject("settings").getJSONArray("vnext").getJSONObject(0).getString("address"))
        assertEquals("reality", stream.getString("security"))
        assertEquals("www.cloudflare.com", settings.getString("serverName"))
        assertEquals("chrome", settings.getString("fingerprint"))
        assertFalse(settings.has("allowInsecure"))
        assertFalse(reality.toString().contains("550e8400"))
        assertFalse(reality.toString().contains("0123456789abcdef"))
    }

    @Test
    fun `unknown reality fields are rejected rather than downgraded`() {
        val candidate = realityTransport()
        candidate.getJSONObject("options").put("allow_insecure", "true")

        val error = assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(JSONArray().put(candidate))
        }

        assertTrue(error.message.orEmpty().contains("Unsupported transport option"))
    }

    @Test
    fun `malformed reality credentials are rejected`() {
        val candidate = realityTransport()
        candidate.getJSONObject("credential").put("auth", "not-a-uuid")

        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(JSONArray().put(candidate))
        }
    }

    private fun realityTransport(): JSONObject = JSONObject()
        .put("id", "vless-reality-primary")
        .put("protocol", "vless_xtls_reality")
        .put("endpoint", "vpn.example.test")
        .put("port", 443)
        .put("network", "tcp")
        .put("priority", 5)
        .put("tls_server_name", "www.cloudflare.com")
        .put(
            "options",
            JSONObject()
                .put("encryption", "none")
                .put("fingerprint", "chrome")
                .put("flow", "xtls-rprx-vision")
                .put("transport", "raw"),
        )
        .put(
            "credential",
            JSONObject()
                .put("auth", "550e8400-e29b-41d4-a716-446655440000")
                .put(
                    "secrets",
                    JSONObject()
                        .put("reality_password", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")
                        .put("reality_short_id", "0123456789abcdef"),
                ),
        )

    private fun hysteriaTransport(): JSONObject = JSONObject()
        .put("id", "hy2-primary")
        .put("protocol", "hysteria2")
        .put("endpoint", "vpn.example.test")
        .put("port", 443)
        .put("network", "udp")
        .put("priority", 10)
        .put("tls_server_name", "vpn.example.test")
        .put("options", JSONObject().put("obfs", "none"))
        .put(
            "credential",
            JSONObject().put("auth", "lease-secret-value").put("secrets", JSONObject()),
        )
}
