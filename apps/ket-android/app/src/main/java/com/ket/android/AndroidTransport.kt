package com.ket.android

import org.json.JSONArray
import org.json.JSONObject

interface AndroidTransport {
    val id: String
    val endpoint: String
    val port: Int
    val priority: Int
    val displayName: String
}

internal object AndroidTransportSelector {
    fun parse(transports: JSONArray): List<AndroidTransport> {
        val supported = buildList {
            for (index in 0 until transports.length()) {
                val candidate = transports.getJSONObject(index)
                when (candidate.optString("protocol")) {
                    "hysteria2" -> add(HysteriaTransport.parse(candidate))
                    "vless_xtls_reality" -> add(RealityTransport.parse(candidate))
                }
            }
        }.sortedWith(compareBy(AndroidTransport::priority, AndroidTransport::id))
        require(supported.isNotEmpty()) { "The server did not offer a supported Android transport" }
        return supported
    }
}

internal fun validateTransportHost(value: String, label: String): String {
    val host = value.trim()
    require(
        host.isNotEmpty() &&
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
