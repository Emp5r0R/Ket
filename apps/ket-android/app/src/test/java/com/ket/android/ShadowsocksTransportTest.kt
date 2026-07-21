package com.ket.android

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Test

class ShadowsocksTransportTest {
    @Test
    fun `strict Shadowsocks 2022 profile is parsed and rendered`() {
        val transport = AndroidTransportSelector.parse(
            JSONArray().put(shadowsocksTransport()),
        ).single() as ShadowsocksTransport
        val config = JSONObject(EngineConfig.shadowsocks(transport, "203.0.113.9", 10808))

        assertEquals("Shadowsocks 2022", transport.displayName)
        assertEquals("203.0.113.9", config.getString("server"))
        assertEquals(20_000, config.getInt("server_port"))
        assertEquals("127.0.0.1", config.getString("local_address"))
        assertEquals(10808, config.getInt("local_port"))
        assertEquals("2022-blake3-aes-256-gcm", config.getString("method"))
        assertEquals("tcp_and_udp", config.getString("mode"))
        assertFalse(transport.toString().contains("AAAAAAAA"))
    }

    @Test
    fun `downgrade options TLS fields and malformed keys are rejected`() {
        val unknown = shadowsocksTransport()
        unknown.getJSONObject("options").put("plugin", "plain")
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(JSONArray().put(unknown))
        }

        val method = shadowsocksTransport()
        method.getJSONObject("options").put("method", "aes-256-gcm")
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(JSONArray().put(method))
        }

        val tls = shadowsocksTransport().put("tls_server_name", "unexpected.example")
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(JSONArray().put(tls))
        }

        val malformed = shadowsocksTransport()
        malformed.getJSONObject("credential").put("auth", "not-base64")
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(JSONArray().put(malformed))
        }

        val secret = shadowsocksTransport()
        secret.getJSONObject("credential").getJSONObject("secrets").put("plugin_key", "secret")
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(JSONArray().put(secret))
        }
    }

    private fun shadowsocksTransport(): JSONObject = JSONObject()
        .put("id", "shadowsocks-2022-primary")
        .put("protocol", "shadowsocks2022")
        .put("endpoint", "vpn.example.test")
        .put("port", 20_000)
        .put("network", "tcp_and_udp")
        .put("priority", 15)
        .put(
            "options",
            JSONObject()
                .put("method", "2022-blake3-aes-256-gcm")
                .put("mode", "tcp_and_udp")
                .put("port_allocation", "lease_slot"),
        )
        .put(
            "credential",
            JSONObject()
                .put("auth", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
                .put("secrets", JSONObject()),
        )
}
