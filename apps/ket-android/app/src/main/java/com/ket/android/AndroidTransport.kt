package com.ket.android

import org.json.JSONArray
import org.json.JSONObject

interface AndroidTransport {
    val id: String
    val endpoint: String
    val port: Int
    val priority: Int
    val displayName: String
    val protocol: KetProtocol
}

interface AndroidXrayTransport : AndroidTransport {
    val tlsServerName: String
    val userId: String
    val fingerprint: String
}

internal object AndroidTransportSelector {
    private const val MAX_TRANSPORTS = 32

    fun parse(transports: JSONArray): List<AndroidTransport> {
        require(transports.length() in 1..MAX_TRANSPORTS) {
            "The server advertised an invalid transport count"
        }
        val supported = buildList {
            for (index in 0 until transports.length()) {
                val candidate = transports.getJSONObject(index)
                validateTransportId(candidate.getString("id"))
                when (candidate.optString("protocol")) {
                    "hysteria2" -> add(HysteriaTransport.parse(candidate))
                    "shadowsocks2022" -> add(ShadowsocksTransport.parse(candidate))
                    "stealth" -> add(StealthTransport.parse(candidate))
                    "vless_xtls_reality" -> add(RealityTransport.parse(candidate))
                    "wire_guard" -> add(WireGuardTlsTransport.parse(candidate))
                    "open_vpn_stunnel" -> add(OpenVpnStunnelTransport.parse(candidate))
                }
            }
        }.sortedWith(compareBy(AndroidTransport::priority, AndroidTransport::id))
        require(supported.isNotEmpty()) { "The server did not offer a supported Android transport" }
        require(supported.map(AndroidTransport::id).toSet().size == supported.size) {
            "The server advertised duplicate transport IDs"
        }
        return supported
    }
}

internal fun validateTransportId(value: String): String = value.also {
    require(
        it.isNotEmpty() &&
            it.length <= 128 &&
            it.all { character ->
                character.code <= 127 &&
                    (character.isLetterOrDigit() || character == '-' || character == '_' || character == '.')
            },
    ) { "Transport ID is invalid" }
}

internal fun validateTransportSecret(value: String, label: String): String = value.also {
    require(it.isNotEmpty() && it.length <= 4_096 && it.none(Char::isISOControl)) {
        "$label is invalid"
    }
}

internal fun validateTransportHost(value: String, label: String): String {
    val host = value.trim()
    require(
        host.isNotEmpty() &&
            host == value &&
            host.length <= 253 &&
            !host.contains("://") &&
            !host.contains('/') &&
            !host.contains('\\') &&
            host.none(Char::isWhitespace),
    ) { "$label is invalid" }
    return host
}

internal fun rejectUnknownKeys(json: JSONObject, known: Set<String>, label: String) {
    val unknown = json.keys().asSequence().firstOrNull { it !in known }
    require(unknown == null) { "Unsupported $label: $unknown" }
}
