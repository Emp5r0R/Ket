package com.ket.android

import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

class EnrollmentResult(
    val token: String,
    val node: AndroidNodeStatus,
    val transports: List<AndroidTransport>,
    internal val manifestJson: String,
) {
    override fun toString(): String =
        "EnrollmentResult(token=[REDACTED], node=${node.displayName}, transports=$transports)"
}
data class SessionTelemetry(
    val node: AndroidNodeStatus,
    val available: Boolean,
    val sent: Long,
    val received: Long,
    val online: Int,
)

internal class KetControlException(
    val statusCode: Int,
    message: String,
) : IllegalStateException(message) {
    val authorizationLost: Boolean = statusCode == 401 || statusCode == 403
}

/** Small platform adapter for the versioned Ket control contract. Secrets never enter logs. */
object KetControlApi : TunnelSessionApi {
    override fun enroll(serverUrl: String, accessCode: String, clientName: String): EnrollmentResult {
        val base = normalizeBaseUrl(serverUrl)
        val validatedAccessCode = validateAccessCode(accessCode)
        val connection = open("$base/v1/sessions").apply {
            requestMethod = "POST"; readTimeout = 15_000; doOutput = true
            setRequestProperty("Content-Type", "application/json")
        }
        connection.outputStream.use { it.write(JSONObject().put("access_code", validatedAccessCode).put("client_name", clientName).toString().toByteArray()) }
        val (code, body) = response(connection)
        requireSuccess(code, body, "Enrollment failed")
        return parseEnrollment(body)
    }

    fun status(serverUrl: String, token: String): SessionTelemetry {
        val connection = open("${normalizeBaseUrl(serverUrl)}/v1/sessions/current").apply {
            requestMethod = "GET"; setRequestProperty("Authorization", "Bearer $token")
        }
        val (code, body) = response(connection)
        requireSuccess(code, body, "Session status unavailable")
        return parseStatus(body)
    }

    override fun renew(serverUrl: String, token: String): Long {
        val connection = open("${normalizeBaseUrl(serverUrl)}/v1/sessions/current").apply {
            requestMethod = "PUT"; setRequestProperty("Authorization", "Bearer $token")
        }
        val (code, body) = response(connection)
        requireSuccess(code, body, "Session renewal failed")
        return JSONObject(body).getLong("expires_at_epoch_seconds")
    }

    override fun release(serverUrl: String, token: String) {
        val connection = open("${normalizeBaseUrl(serverUrl)}/v1/sessions/current").apply {
            requestMethod = "DELETE"; setRequestProperty("Authorization", "Bearer $token")
        }
        val (code, body) = response(connection)
        requireSuccess(code, body, "Session release failed")
    }

    internal fun parseEnrollment(body: String): EnrollmentResult {
        val json = JSONObject(body)
        return EnrollmentResult(
            token = json.getString("session_token").also { require(it.isNotBlank()) { "Session token is missing" } },
            node = AndroidNodeStatusParser.parse(json.getJSONObject("node")),
            transports = AndroidTransportSelector.parse(json.getJSONArray("transports")),
            manifestJson = json.toString(),
        )
    }

    internal fun parseStatus(body: String): SessionTelemetry {
        val json = JSONObject(body)
        val traffic = json.getJSONObject("traffic")
        return SessionTelemetry(
            node = AndroidNodeStatusParser.parse(json.getJSONObject("node")),
            available = traffic.getBoolean("available"),
            sent = nonNegativeLong(traffic, "bytes_sent"),
            received = nonNegativeLong(traffic, "bytes_received"),
            online = traffic.getInt("online_connections").also {
                require(it >= 0) { "Online connection count is invalid" }
            },
        )
    }

    internal fun normalizeBaseUrl(serverUrl: String): String {
        val url = URL(serverUrl.trim().trimEnd('/'))
        val localHttp = url.protocol == "http" && (url.host == "localhost" || url.host == "127.0.0.1" || url.host == "::1")
        require(url.protocol == "https" || localHttp) { "Use HTTPS for a remote server" }
        require(url.userInfo == null && url.query == null && url.ref == null && url.path.isBlank()) { "Server URL must not contain credentials, a path, query, or fragment" }
        return url.toExternalForm().trimEnd('/')
    }

    internal fun validateAccessCode(accessCode: String): String = accessCode.also {
        require(it.length == 32 && it.all { character -> character.isLetterOrDigit() && character.code <= 127 }) {
            "Access code must contain exactly 32 ASCII letters or digits"
        }
    }

    private fun open(url: String): HttpURLConnection = (URL(url).openConnection() as HttpURLConnection).apply {
        connectTimeout = 5_000
        readTimeout = 10_000
        instanceFollowRedirects = false
        setRequestProperty("Accept", "application/json")
    }

    private fun response(connection: HttpURLConnection): Pair<Int, String> {
        return try {
            val code = connection.responseCode
            val stream = if (code in 200..299) connection.inputStream else connection.errorStream
            code to (stream?.bufferedReader()?.use { it.readText() } ?: "")
        } finally {
            connection.disconnect()
        }
    }

    private fun errorMessage(body: String, fallback: String): String {
        if (body.isBlank()) return fallback
        return runCatching { JSONObject(body).optString("message", fallback) }.getOrDefault(fallback)
    }

    private fun requireSuccess(code: Int, body: String, fallback: String) {
        if (code !in 200..299) throw KetControlException(code, errorMessage(body, fallback))
    }

    private fun nonNegativeLong(json: JSONObject, key: String): Long = json.getLong(key).also {
        require(it >= 0) { "$key is invalid" }
    }
}
