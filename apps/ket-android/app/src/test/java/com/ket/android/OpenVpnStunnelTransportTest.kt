package com.ket.android

import java.util.Base64
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class OpenVpnStunnelTransportTest {
    @Test
    fun `parses the hardened profile and renders management-only credentials`() {
        val profile = validTransport()
        val transport = AndroidTransportSelector.parse(JSONArray().put(profile)).single()
            as OpenVpnStunnelTransport
        val config = OpenVpnAndroidConfig.render(transport, 18443, "/data/user/0/com.ket.android/cache/ovpn.sock")
            .toString(Charsets.UTF_8)

        assertEquals("OpenVPN TLS", transport.displayName)
        assertTrue(config.contains("remote 127.0.0.1 18443"))
        assertTrue(config.contains("management-client"))
        assertTrue(config.contains("management-query-passwords"))
        assertTrue(config.contains("verify-x509-name openvpn.example.test name"))
        assertTrue(config.contains("tls-version-min 1.2"))
        assertTrue(config.contains("allow-compression no"))
        assertTrue(config.contains("auth-retry none"))
        assertFalse(config.contains(transport.username))
        assertFalse(config.contains(transport.password))
        assertFalse(transport.toString().contains(transport.password))
    }

    @Test
    fun `rejects downgrade options and mismatched TLS identity`() {
        val downgrade = validTransport().apply {
            getJSONObject("options").put("tls_minimum", "1.0")
        }
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(JSONArray().put(downgrade))
        }

        val mismatch = validTransport().put("endpoint", "other.example.test")
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(JSONArray().put(mismatch))
        }
    }

    @Test
    fun `rejects malformed material and unscoped credentials`() {
        val malformed = validTransport().apply {
            getJSONObject("credential").getJSONObject("secrets")
                .put("tls_crypt_key_b64", Base64.getEncoder().encodeToString("not a key".toByteArray()))
        }
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(JSONArray().put(malformed))
        }

        val unscoped = validTransport().apply {
            getJSONObject("credential").put("auth", "Z23456789012ABCDEFGHIJKLMNOPQRSTUVWXYZ123456")
        }
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(JSONArray().put(unscoped))
        }
    }

    @Test
    fun `accepts modern CA chains and rejects oversized material`() {
        fun encodedCertificate(bodyLines: Int): String {
            val body = List(bodyLines) { "A".repeat(64) }.joinToString("\n")
            val pem = "-----BEGIN CERTIFICATE-----\n$body\n-----END CERTIFICATE-----\n"
            return Base64.getEncoder().encodeToString(pem.toByteArray())
        }

        val bounded = validTransport().apply {
            getJSONObject("credential").getJSONObject("secrets")
                .put("stunnel_ca_certificate_pem_b64", encodedCertificate(54))
        }
        AndroidTransportSelector.parse(JSONArray().put(bounded))

        val oversized = validTransport().apply {
            getJSONObject("credential").getJSONObject("secrets")
                .put("stunnel_ca_certificate_pem_b64", encodedCertificate(128))
        }
        assertThrows(IllegalArgumentException::class.java) {
            AndroidTransportSelector.parse(JSONArray().put(oversized))
        }
    }

    private fun validTransport(): JSONObject {
        val encode = { value: String -> Base64.getEncoder().encodeToString(value.toByteArray()) }
        return JSONObject()
            .put("id", "openvpn-stunnel-primary")
            .put("protocol", "open_vpn_stunnel")
            .put("endpoint", "openvpn.example.test")
            .put("port", 443)
            .put("network", "tcp")
            .put("priority", 8)
            .put("tls_server_name", "openvpn.example.test")
            .put(
                "options",
                JSONObject()
                    .put("auth_mode", "session_token")
                    .put("cipher", "aes_256_gcm")
                    .put("remote_cert_tls", "server")
                    .put("tls_crypt", "v1")
                    .put("tls_minimum", "1.2")
                    .put("transport", "stunnel_tls"),
            )
            .put(
                "credential",
                JSONObject()
                    .put("auth", "AbCdEf123456ABCDEFGHIJKLMNOPQRSTUVWXYZ123456")
                    .put(
                        "secrets",
                        JSONObject()
                            .put("username", "AbCdEf123456")
                            .put(
                                "ca_certificate_pem_b64",
                                encode("-----BEGIN CERTIFICATE-----\ndGVzdC1jYQ==\n-----END CERTIFICATE-----\n"),
                            )
                            .put(
                                "stunnel_ca_certificate_pem_b64",
                                encode("-----BEGIN CERTIFICATE-----\ndGVzdC1vdXRlcg==\n-----END CERTIFICATE-----\n"),
                            )
                            .put(
                                "tls_crypt_key_b64",
                                encode(
                                    "-----BEGIN OpenVPN Static key V1-----\n" +
                                        "dGVzdC10bHMtY3J5cHQta2V5\n" +
                                        "-----END OpenVPN Static key V1-----\n",
                                ),
                            ),
                    ),
            )
    }
}
