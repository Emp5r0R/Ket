package com.ket.android

import org.json.JSONArray
import org.json.JSONObject

private val knownOptions = setOf("obfs", "gecko_min_packet_size", "gecko_max_packet_size")
private val knownSecrets = setOf("obfs_password")

class HysteriaTransport private constructor(
    override val id: String,
    override val endpoint: String,
    override val port: Int,
    override val priority: Int,
    val tlsServerName: String,
    val auth: String,
    val obfuscation: HysteriaObfuscation,
) : AndroidTransport {
    override val displayName: String = "Hysteria 2"

    override fun toString(): String =
        "HysteriaTransport(id=$id, endpoint=$endpoint, port=$port, tlsServerName=$tlsServerName, auth=[REDACTED], obfuscation=${obfuscation.redactedName})"

    companion object {
        fun select(transports: JSONArray): HysteriaTransport {
            val candidate = (0 until transports.length())
                .asSequence()
                .map { transports.getJSONObject(it) }
                .firstOrNull { it.optString("protocol") == "hysteria2" }
                ?: throw IllegalStateException("The server did not offer Hysteria2")
            return parse(candidate)
        }

        internal fun parse(json: JSONObject): HysteriaTransport {
            require(json.getString("protocol") == "hysteria2") { "Unsupported Android transport" }
            require(json.getString("network") == "udp") { "Hysteria2 must use UDP" }
            val id = json.getString("id").also { require(it.isNotBlank()) { "Transport ID is missing" } }
            val endpoint = validateHost(json.getString("endpoint"), "Transport endpoint")
            val port = json.getInt("port").also { require(it in 1..65535) { "Transport port is invalid" } }
            val priority = json.optInt("priority", 100).also { require(it in 0..65535) { "Transport priority is invalid" } }
            val sni = validateHost(json.getString("tls_server_name"), "TLS server name")
            val options = json.optJSONObject("options") ?: JSONObject()
            rejectUnknownKeys(options, knownOptions, "transport option")
            val credential = json.optJSONObject("credential")
                ?: throw IllegalArgumentException("Transport credential is missing")
            val auth = credential.optString("auth").also {
                require(it.isNotBlank()) { "Transport credential is empty" }
            }
            val secrets = credential.optJSONObject("secrets") ?: JSONObject()
            rejectUnknownKeys(secrets, knownSecrets, "transport credential")
            val obfs = when (val mode = options.optString("obfs", "none")) {
                "none" -> {
                    require(!secrets.has("obfs_password")) {
                        "Obfuscation password was supplied without an obfuscation mode"
                    }
                    HysteriaObfuscation.None
                }
                "salamander" -> HysteriaObfuscation.Salamander(requiredSecret(secrets))
                "gecko" -> {
                    val minimum = packetSize(options, "gecko_min_packet_size", 512)
                    val maximum = packetSize(options, "gecko_max_packet_size", 1200)
                    require(minimum <= maximum && maximum <= 2048) { "Gecko packet size bounds are invalid" }
                    HysteriaObfuscation.Gecko(requiredSecret(secrets), minimum, maximum)
                }
                else -> throw IllegalArgumentException("Unsupported Hysteria2 obfuscation mode: $mode")
            }
            return HysteriaTransport(id, endpoint, port, priority, sni, auth, obfs)
        }

        private fun validateHost(value: String, label: String): String {
            return validateTransportHost(value, label)
        }

        private fun rejectUnknownKeys(json: JSONObject, known: Set<String>, label: String) {
            com.ket.android.rejectUnknownKeys(json, known, label)
        }

        private fun requiredSecret(secrets: JSONObject): String =
            secrets.optString("obfs_password").also {
                require(it.isNotBlank()) { "Obfuscation password is missing" }
            }

        private fun packetSize(options: JSONObject, key: String, default: Int): Int {
            if (!options.has(key)) return default
            return options.getString(key).toIntOrNull()
                ?: throw IllegalArgumentException("$key is invalid")
        }
    }
}

sealed class HysteriaObfuscation(internal val redactedName: String) {
    data object None : HysteriaObfuscation("none")
    class Salamander(internal val password: String) : HysteriaObfuscation("salamander")
    class Gecko(
        internal val password: String,
        internal val minimumPacketSize: Int,
        internal val maximumPacketSize: Int,
    ) : HysteriaObfuscation("gecko")
}
