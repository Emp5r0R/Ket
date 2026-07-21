package com.ket.android

import java.util.Base64
import org.json.JSONObject

private val wireGuardOptions = setOf(
    "address_allocation",
    "allowed_ips",
    "client_address",
    "keepalive_seconds",
    "mtu",
    "path_prefix",
    "remote_address",
    "transport",
)
private val wireGuardSecrets = setOf("preshared_key", "server_public_key")
private val pathPrefixPattern = Regex("^[A-Za-z0-9_-]{16,96}$")
private val remoteAddressPattern = Regex("^[A-Za-z0-9.-]{1,253}:[0-9]{1,5}$")

class WireGuardTlsTransport private constructor(
    override val id: String,
    override val endpoint: String,
    override val port: Int,
    override val priority: Int,
    val tlsServerName: String,
    val clientAddress: String,
    val pathPrefix: String,
    val remoteAddress: String,
    val privateKey: String,
    val presharedKey: String,
    val serverPublicKey: String,
) : AndroidTransport {
    override val displayName: String = "WireGuard TLS"
    override val protocol: KetProtocol = KetProtocol.WireGuardTls

    override fun toString(): String =
        "WireGuardTlsTransport(id=$id, endpoint=$endpoint, port=$port, tlsServerName=$tlsServerName, clientAddress=$clientAddress, pathPrefix=[REDACTED], remoteAddress=$remoteAddress, keys=[REDACTED])"

    companion object {
        internal fun parse(json: JSONObject): WireGuardTlsTransport {
            require(json.getString("protocol") == "wire_guard") { "Unsupported Android transport" }
            require(json.getString("network") == "tcp") { "WireGuard TLS must use TCP" }
            val id = validateTransportId(json.getString("id"))
            val endpoint = validateTransportHost(json.getString("endpoint"), "Transport endpoint")
            val port = json.getInt("port").also { require(it in 1..65535) { "Transport port is invalid" } }
            val priority = json.optInt("priority", 100).also {
                require(it in 0..65535) { "Transport priority is invalid" }
            }
            val sni = validateTransportHost(json.getString("tls_server_name"), "WireGuard TLS server name")
            require(sni.any(Char::isLetter)) { "WireGuard TLS server name must be a hostname" }
            val options = json.optJSONObject("options") ?: JSONObject()
            rejectUnknownKeys(options, wireGuardOptions, "transport option")
            require(options.optString("address_allocation") == "lease_slot") {
                "Unsupported WireGuard address allocation"
            }
            require(options.optString("allowed_ips") == "0.0.0.0/0") { "Unsupported WireGuard routes" }
            require(options.optString("keepalive_seconds") == "25") { "Unsupported WireGuard keepalive" }
            require(options.optString("mtu") == "1280") { "Unsupported WireGuard MTU" }
            require(options.optString("transport") == "websocket_tls") { "Unsupported WireGuard carrier" }
            val clientAddress = options.optString("client_address").also {
                require(validClientAddress(it)) { "WireGuard client address is invalid" }
            }
            val pathPrefix = options.optString("path_prefix").also {
                require(pathPrefixPattern.matches(it)) { "WireGuard path prefix is invalid" }
            }
            val remoteAddress = options.optString("remote_address").also {
                require(validRemoteAddress(it)) { "WireGuard remote address is invalid" }
            }
            val credential = json.optJSONObject("credential")
                ?: throw IllegalArgumentException("Transport credential is missing")
            val privateKey = validWireGuardKey(credential.optString("auth"), "WireGuard private key")
            val secrets = credential.optJSONObject("secrets") ?: JSONObject()
            rejectUnknownKeys(secrets, wireGuardSecrets, "transport credential")
            val presharedKey = validWireGuardKey(
                secrets.optString("preshared_key"),
                "WireGuard preshared key",
            )
            val serverPublicKey = validWireGuardKey(
                secrets.optString("server_public_key"),
                "WireGuard server public key",
            )
            return WireGuardTlsTransport(
                id,
                endpoint,
                port,
                priority,
                sni,
                clientAddress,
                pathPrefix,
                remoteAddress,
                privateKey,
                presharedKey,
                serverPublicKey,
            )
        }

        private fun validClientAddress(value: String): Boolean {
            val octets = value.split('.').mapNotNull(String::toIntOrNull)
            if (octets.size != 4 || octets.any { it !in 0..255 } || octets[0] != 10 || octets[1] != 66) {
                return false
            }
            val host = octets[2] * 256 + octets[3]
            return host in 2..65534
        }

        private fun validRemoteAddress(value: String): Boolean {
            if (!remoteAddressPattern.matches(value)) return false
            val (host, port) = value.split(':', limit = 2)
            return host.split('.').all {
                it.isNotEmpty() && it.length <= 63 && !it.startsWith('-') && !it.endsWith('-')
            } && port.toIntOrNull() in 1..65535
        }

        private fun validWireGuardKey(value: String, label: String): String {
            val key = validateTransportSecret(value, label)
            val decoded = runCatching { Base64.getDecoder().decode(key) }
                .getOrElse { throw IllegalArgumentException("$label is invalid") }
            require(decoded.size == 32) { "$label must contain 32 bytes" }
            decoded.fill(0)
            return key
        }
    }
}
