package com.ket.android

import org.json.JSONObject

private val stealthOptions = setOf("encryption", "fingerprint", "mode", "path", "security", "transport")
private val stealthFingerprints = setOf("chrome", "firefox", "safari", "ios", "android", "edge", "random")
private val stealthUuidPattern = Regex("^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")

class StealthTransport private constructor(
    override val id: String,
    override val endpoint: String,
    override val port: Int,
    override val priority: Int,
    override val tlsServerName: String,
    override val userId: String,
    override val fingerprint: String,
    val path: String,
) : AndroidXrayTransport {
    override val displayName: String = "HTTPS Stealth"

    override fun toString(): String =
        "StealthTransport(id=$id, endpoint=$endpoint, port=$port, tlsServerName=$tlsServerName, userId=[REDACTED], fingerprint=$fingerprint, path=[REDACTED])"

    companion object {
        internal fun parse(json: JSONObject): StealthTransport {
            require(json.getString("protocol") == "stealth") { "Unsupported Android transport" }
            require(json.getString("network") == "tcp") { "HTTPS Stealth must use TCP" }
            val id = validateTransportId(json.getString("id"))
            val endpoint = validateTransportHost(json.getString("endpoint"), "Transport endpoint")
            val port = json.getInt("port").also { require(it in 1..65535) { "Transport port is invalid" } }
            val priority = json.optInt("priority", 100).also { require(it in 0..65535) { "Transport priority is invalid" } }
            val sni = validateTransportHost(json.getString("tls_server_name"), "Stealth server name")
            require(sni.any(Char::isLetter)) { "Stealth server name must be a hostname" }
            val options = json.optJSONObject("options") ?: JSONObject()
            rejectUnknownKeys(options, stealthOptions, "transport option")
            require(options.optString("encryption") == "none") { "Unsupported VLESS encryption" }
            require(options.optString("transport") == "xhttp") { "Unsupported stealth transport" }
            require(options.optString("security") == "tls") { "Unsupported stealth security" }
            require(options.optString("mode") == "packet-up") { "Unsupported XHTTP mode" }
            val fingerprint = options.optString("fingerprint").also {
                require(it in stealthFingerprints) { "Stealth fingerprint is invalid" }
            }
            val path = options.optString("path").also {
                require(validPath(it)) { "Stealth path is invalid" }
            }
            val credential = json.optJSONObject("credential")
                ?: throw IllegalArgumentException("Transport credential is missing")
            val userId = credential.optString("auth").also {
                require(stealthUuidPattern.matches(it)) { "VLESS user ID is invalid" }
            }
            val secrets = credential.optJSONObject("secrets") ?: JSONObject()
            rejectUnknownKeys(secrets, emptySet(), "transport credential")
            return StealthTransport(id, endpoint, port, priority, sni, userId, fingerprint, path)
        }

        private fun validPath(path: String): Boolean =
            path.length in 16..128 &&
                path.startsWith('/') &&
                !path.endsWith('/') &&
                !path.contains("//") &&
                path.all {
                    it.code <= 127 && (it.isLetterOrDigit() || it == '/' || it == '-' || it == '_')
                }
    }
}
