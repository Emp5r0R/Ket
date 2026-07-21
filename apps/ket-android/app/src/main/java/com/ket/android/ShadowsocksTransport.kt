package com.ket.android

import java.util.Base64
import org.json.JSONObject

internal const val SHADOWSOCKS_METHOD = "2022-blake3-aes-256-gcm"
private val shadowsocksOptions = setOf("method", "mode", "port_allocation")

class ShadowsocksTransport private constructor(
    override val id: String,
    override val endpoint: String,
    override val port: Int,
    override val priority: Int,
    val key: String,
) : AndroidTransport {
    override val displayName: String = "Shadowsocks 2022"
    override val protocol: KetProtocol = KetProtocol.Shadowsocks2022

    override fun toString(): String =
        "ShadowsocksTransport(id=$id, endpoint=$endpoint, port=$port, key=[REDACTED])"

    companion object {
        internal fun parse(json: JSONObject): ShadowsocksTransport {
            require(json.getString("protocol") == "shadowsocks2022") {
                "Unsupported Android transport"
            }
            require(json.getString("network") == "tcp_and_udp") {
                "Shadowsocks 2022 must use TCP and UDP"
            }
            require(!json.has("tls_server_name") || json.isNull("tls_server_name")) {
                "Shadowsocks 2022 must not declare a TLS server name"
            }
            val id = validateTransportId(json.getString("id"))
            val endpoint = validateTransportHost(json.getString("endpoint"), "Transport endpoint")
            val port = json.getInt("port").also {
                require(it in 1..65535) { "Transport port is invalid" }
            }
            val priority = json.optInt("priority", 100).also {
                require(it in 0..65535) { "Transport priority is invalid" }
            }
            val options = json.optJSONObject("options") ?: JSONObject()
            rejectUnknownKeys(options, shadowsocksOptions, "transport option")
            require(options.optString("method") == SHADOWSOCKS_METHOD) {
                "Unsupported Shadowsocks 2022 method"
            }
            require(options.optString("mode") == "tcp_and_udp") {
                "Unsupported Shadowsocks 2022 mode"
            }
            require(options.optString("port_allocation") == "lease_slot") {
                "Unsupported Shadowsocks 2022 port allocation"
            }
            val credential = json.optJSONObject("credential")
                ?: throw IllegalArgumentException("Transport credential is missing")
            val key = validateTransportSecret(
                credential.optString("auth"),
                "Shadowsocks 2022 key",
            )
            val decoded = runCatching { Base64.getDecoder().decode(key) }
                .getOrElse { throw IllegalArgumentException("Shadowsocks 2022 key is invalid") }
            require(decoded.size == 32) { "Shadowsocks 2022 key must contain 32 bytes" }
            decoded.fill(0)
            val secrets = credential.optJSONObject("secrets") ?: JSONObject()
            rejectUnknownKeys(secrets, emptySet(), "transport credential")
            return ShadowsocksTransport(id, endpoint, port, priority, key)
        }
    }
}
