package com.ket.android

import org.json.JSONObject

private val realityOptions = setOf("encryption", "fingerprint", "flow", "transport")
private val realitySecrets = setOf("reality_password", "reality_short_id")
private val realityFingerprints = setOf("chrome", "firefox", "safari", "ios", "android", "edge", "random")
private val uuidPattern = Regex("^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")
private val passwordPattern = Regex("^[A-Za-z0-9_-]{43}$")
private val shortIdPattern = Regex("^[0-9a-fA-F]{16}$")

class RealityTransport private constructor(
    override val id: String,
    override val endpoint: String,
    override val port: Int,
    override val priority: Int,
    val tlsServerName: String,
    val userId: String,
    val password: String,
    val shortId: String,
    val fingerprint: String,
) : AndroidTransport {
    override val displayName: String = "VLESS + REALITY"

    override fun toString(): String =
        "RealityTransport(id=$id, endpoint=$endpoint, port=$port, tlsServerName=$tlsServerName, userId=[REDACTED], password=[REDACTED], shortId=[REDACTED], fingerprint=$fingerprint)"

    companion object {
        internal fun parse(json: JSONObject): RealityTransport {
            require(json.getString("protocol") == "vless_xtls_reality") { "Unsupported Android transport" }
            require(json.getString("network") == "tcp") { "VLESS + REALITY must use TCP" }
            val id = validateTransportId(json.getString("id"))
            val endpoint = validateTransportHost(json.getString("endpoint"), "Transport endpoint")
            val port = json.getInt("port").also { require(it in 1..65535) { "Transport port is invalid" } }
            val priority = json.optInt("priority", 100).also { require(it in 0..65535) { "Transport priority is invalid" } }
            val sni = validateTransportHost(json.getString("tls_server_name"), "Reality server name")
            require(sni.any(Char::isLetter)) { "Reality server name must be a hostname" }
            val options = json.optJSONObject("options") ?: JSONObject()
            rejectUnknownKeys(options, realityOptions, "transport option")
            require(options.optString("encryption") == "none") { "Unsupported VLESS encryption" }
            require(options.optString("flow") == "xtls-rprx-vision") { "Unsupported VLESS flow" }
            require(options.optString("transport") == "raw") { "Unsupported VLESS transport" }
            val fingerprint = options.optString("fingerprint").also {
                require(it in realityFingerprints) { "Reality fingerprint is invalid" }
            }
            val credential = json.optJSONObject("credential")
                ?: throw IllegalArgumentException("Transport credential is missing")
            val userId = credential.optString("auth").also {
                require(uuidPattern.matches(it)) { "VLESS user ID is invalid" }
            }
            val secrets = credential.optJSONObject("secrets") ?: JSONObject()
            rejectUnknownKeys(secrets, realitySecrets, "transport credential")
            val password = secrets.optString("reality_password").also {
                require(passwordPattern.matches(it)) { "Reality password is invalid" }
            }
            val shortId = secrets.optString("reality_short_id").also {
                require(shortIdPattern.matches(it)) { "Reality short ID is invalid" }
            }
            return RealityTransport(
                id,
                endpoint,
                port,
                priority,
                sni,
                userId,
                password,
                shortId.lowercase(),
                fingerprint,
            )
        }
    }
}
