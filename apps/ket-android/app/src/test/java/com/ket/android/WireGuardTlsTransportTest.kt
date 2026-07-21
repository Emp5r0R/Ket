package com.ket.android

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Test

class WireGuardTlsTransportTest {
    @Test
    fun `strict WireGuard TLS profile is parsed and rendered`() {
        val transport = AndroidTransportSelector.parse(
            JSONArray().put(wireGuardTransport()),
        ).single() as WireGuardTlsTransport
        val config = JSONObject(EngineConfig.wireGuard(transport, 51821, 10808))
        val settings = config.getJSONArray("outbounds").getJSONObject(0).getJSONObject("settings")
        val peer = settings.getJSONArray("peers").getJSONObject(0)

        assertEquals("WireGuard TLS", transport.displayName)
        assertEquals("10.66.0.2/32", settings.getJSONArray("address").getString(0))
        assertEquals("127.0.0.1:51821", peer.getString("endpoint"))
        assertEquals("0.0.0.0/0", peer.getJSONArray("allowedIPs").getString(0))
        assertEquals(25, peer.getInt("keepAlive"))
        assertFalse(transport.toString().contains("AQEBAQ"))
    }

    @Test
    fun `downgrade options malformed leases and unknown secrets are rejected`() {
        val insecure = wireGuardTransport()
        insecure.getJSONObject("options").put("allow_insecure", "true")
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(JSONArray().put(insecure))
        }
        val carrier = wireGuardTransport()
        carrier.getJSONObject("options").put("transport", "websocket")
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(JSONArray().put(carrier))
        }
        val address = wireGuardTransport()
        address.getJSONObject("options").put("client_address", "10.66.0.1")
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(JSONArray().put(address))
        }
        val key = wireGuardTransport()
        key.getJSONObject("credential").put("auth", "invalid")
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(JSONArray().put(key))
        }
    }

    private fun wireGuardTransport(): JSONObject = JSONObject()
        .put("id", "wireguard-tls-primary")
        .put("protocol", "wire_guard")
        .put("endpoint", "wg.example.test")
        .put("port", 443)
        .put("network", "tcp")
        .put("priority", 2)
        .put("tls_server_name", "wg.example.test")
        .put(
            "options",
            JSONObject()
                .put("address_allocation", "lease_slot")
                .put("allowed_ips", "0.0.0.0/0")
                .put("client_address", "10.66.0.2")
                .put("keepalive_seconds", "25")
                .put("mtu", "1280")
                .put("path_prefix", "ket-wireguard-test")
                .put("remote_address", "wireguard-agent:51820")
                .put("transport", "websocket_tls"),
        )
        .put(
            "credential",
            JSONObject()
                .put("auth", "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=")
                .put(
                    "secrets",
                    JSONObject()
                        .put("preshared_key", "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=")
                        .put("server_public_key", "AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM="),
                ),
        )
}
