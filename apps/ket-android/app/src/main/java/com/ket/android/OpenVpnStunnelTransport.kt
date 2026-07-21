package com.ket.android

import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction
import java.util.Base64
import org.json.JSONObject

private val openVpnOptions = setOf(
    "auth_mode",
    "cipher",
    "remote_cert_tls",
    "tls_crypt",
    "tls_minimum",
    "transport",
)
private val openVpnSecrets = setOf(
    "ca_certificate_pem_b64",
    "stunnel_ca_certificate_pem_b64",
    "tls_crypt_key_b64",
    "username",
)

internal class OpenVpnStunnelTransport private constructor(
    override val id: String,
    override val endpoint: String,
    override val port: Int,
    override val priority: Int,
    val tlsServerName: String,
    val username: String,
    val password: String,
    val caCertificate: String,
    val stunnelCaCertificate: String,
    val tlsCryptKey: String,
) : AndroidTransport {
    override val displayName: String = "OpenVPN TLS"

    override fun toString(): String =
        "OpenVpnStunnelTransport(id=$id, endpoint=$endpoint, port=$port, tlsServerName=$tlsServerName, credentials=[REDACTED])"

    companion object {
        internal fun parse(json: JSONObject): OpenVpnStunnelTransport {
            require(json.getString("protocol") == "open_vpn_stunnel") {
                "Unsupported Android transport"
            }
            require(json.getString("network") == "tcp") { "OpenVPN TLS must use TCP" }
            val id = validateTransportId(json.getString("id"))
            val endpoint = validateTransportHost(json.getString("endpoint"), "Transport endpoint")
            val port = json.getInt("port").also {
                require(it in 1..65535) { "Transport port is invalid" }
            }
            val priority = json.optInt("priority", 100).also {
                require(it in 0..65535) { "Transport priority is invalid" }
            }
            val sni = validateDnsName(
                validateTransportHost(json.getString("tls_server_name"), "OpenVPN TLS server name"),
                "OpenVPN TLS server name",
            )
            if (!isIpLiteral(endpoint)) {
                require(endpoint.equals(sni, ignoreCase = true)) {
                    "OpenVPN endpoint and TLS identity must match"
                }
            }

            val options = json.optJSONObject("options") ?: JSONObject()
            rejectUnknownKeys(options, openVpnOptions, "transport option")
            mapOf(
                "auth_mode" to "session_token",
                "cipher" to "aes_256_gcm",
                "remote_cert_tls" to "server",
                "tls_crypt" to "v1",
                "tls_minimum" to "1.2",
                "transport" to "stunnel_tls",
            ).forEach { (key, expected) ->
                require(options.optString(key) == expected) { "Unsupported OpenVPN $key" }
            }

            val credential = json.optJSONObject("credential")
                ?: throw IllegalArgumentException("OpenVPN credential is missing")
            val password = validateTransportSecret(credential.optString("auth"), "OpenVPN password")
            val secrets = credential.optJSONObject("secrets") ?: JSONObject()
            rejectUnknownKeys(secrets, openVpnSecrets, "transport credential")
            require(openVpnSecrets.all(secrets::has)) { "OpenVPN credential is incomplete" }
            val username = validateTransportSecret(secrets.optString("username"), "OpenVPN username")
            require(
                username.length == 12 &&
                    username.all { it.code <= 127 && it.isLetterOrDigit() } &&
                    password.length == 44 &&
                    password.all { it.code <= 127 && it.isLetterOrDigit() } &&
                    password.startsWith(username),
            ) { "OpenVPN scoped credential is invalid" }

            return OpenVpnStunnelTransport(
                id = id,
                endpoint = endpoint,
                port = port,
                priority = priority,
                tlsServerName = sni,
                username = username,
                password = password,
                caCertificate = decodePem(
                    secrets.getString("ca_certificate_pem_b64"),
                    "OpenVPN CA certificate",
                    "-----BEGIN CERTIFICATE-----",
                    "-----END CERTIFICATE-----",
                ),
                stunnelCaCertificate = decodePem(
                    secrets.getString("stunnel_ca_certificate_pem_b64"),
                    "OpenVPN carrier CA certificate",
                    "-----BEGIN CERTIFICATE-----",
                    "-----END CERTIFICATE-----",
                ),
                tlsCryptKey = decodePem(
                    secrets.getString("tls_crypt_key_b64"),
                    "OpenVPN tls-crypt key",
                    "-----BEGIN OpenVPN Static key V1-----",
                    "-----END OpenVPN Static key V1-----",
                ),
            )
        }

        private fun decodePem(encoded: String, label: String, begin: String, end: String): String {
            validateTransportSecret(encoded, label)
            val decoded = runCatching { Base64.getDecoder().decode(encoded) }
                .getOrElse { throw IllegalArgumentException("$label is not base64") }
            require(decoded.size in 1..3 * 1024) { "$label has an invalid size" }
            val material = try {
                Charsets.UTF_8.newDecoder()
                    .onMalformedInput(CodingErrorAction.REPORT)
                    .onUnmappableCharacter(CodingErrorAction.REPORT)
                    .decode(ByteBuffer.wrap(decoded))
                    .toString()
            } catch (_: Exception) {
                throw IllegalArgumentException("$label is not UTF-8")
            } finally {
                decoded.fill(0)
            }
            val trimmed = material.trim()
            require(
                trimmed.startsWith(begin) &&
                    trimmed.endsWith(end) &&
                    '\u0000' !in trimmed &&
                    trimmed.lineSequence().all { it.length <= 256 },
            ) { "$label contains invalid key material" }
            return "$trimmed\n"
        }

        private fun validateDnsName(value: String, label: String): String = value.also {
            require(
                it.length <= 253 &&
                    !it.startsWith('.') &&
                    !it.endsWith('.') &&
                    it.split('.').all { part ->
                        part.isNotEmpty() &&
                            part.length <= 63 &&
                            !part.startsWith('-') &&
                            !part.endsWith('-') &&
                            part.all { character ->
                                character.code <= 127 &&
                                    (character.isLetterOrDigit() || character == '-')
                            }
                    },
            ) { "$label is invalid" }
        }

        private fun isIpLiteral(value: String): Boolean =
            ':' in value || value.split('.').let { parts ->
                parts.size == 4 && parts.all { part ->
                    part.isNotEmpty() && part.length <= 3 && part.toIntOrNull() in 0..255
                }
            }
    }
}
