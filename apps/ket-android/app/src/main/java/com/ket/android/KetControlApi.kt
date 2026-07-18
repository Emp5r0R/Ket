package com.ket.android

import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

class EnrollmentResult(
    val token: String,
    val node: String,
    val country: String,
    val transport: HysteriaTransport,
) {
    override fun toString(): String =
        "EnrollmentResult(token=[REDACTED], node=$node, country=$country, transport=$transport)"
}
data class SessionTelemetry(val node: String, val sent: Long, val received: Long, val online: Int, val capacity: Double)

/** Small platform adapter for the versioned Ket control contract. Secrets never enter logs. */
object KetControlApi {
    fun enroll(serverUrl: String, accessCode: String, clientName: String): EnrollmentResult {
        val base = normalizeBaseUrl(serverUrl)
        require(accessCode.length == 32) { "Access code must be 32 characters" }
        val connection = open("$base/v1/sessions").apply {
            requestMethod = "POST"; readTimeout = 15_000; doOutput = true
            setRequestProperty("Content-Type", "application/json")
        }
        connection.outputStream.use { it.write(JSONObject().put("access_code", accessCode).put("client_name", clientName).toString().toByteArray()) }
        val (code, body) = response(connection)
        if (code !in 200..299) throw IllegalStateException(errorMessage(body, "Enrollment failed"))
        return parseEnrollment(body)
    }

    fun status(serverUrl: String, token: String): SessionTelemetry {
        val connection = open("${normalizeBaseUrl(serverUrl)}/v1/sessions/current").apply {
            requestMethod = "GET"; setRequestProperty("Authorization", "Bearer $token")
        }
        val (code, body) = response(connection)
        if (code !in 200..299) throw IllegalStateException(errorMessage(body, "Session status unavailable"))
        val json = JSONObject(body); val node = json.getJSONObject("node"); val traffic = json.getJSONObject("traffic")
        return SessionTelemetry(
            node.getString("display_name"),
            traffic.optLong("bytes_sent"),
            traffic.optLong("bytes_received"),
            traffic.optInt("online_connections"),
            node.getDouble("capacity_percent"),
        )
    }

    fun renew(serverUrl: String, token: String): Long {
        val connection = open("${normalizeBaseUrl(serverUrl)}/v1/sessions/current").apply {
            requestMethod = "PUT"; setRequestProperty("Authorization", "Bearer $token")
        }
        val (code, body) = response(connection)
        if (code !in 200..299) throw IllegalStateException(errorMessage(body, "Session renewal failed"))
        return JSONObject(body).getLong("expires_at_epoch_seconds")
    }

    fun release(serverUrl: String, token: String) {
        val connection = open("${normalizeBaseUrl(serverUrl)}/v1/sessions/current").apply {
            requestMethod = "DELETE"; setRequestProperty("Authorization", "Bearer $token")
        }
        val (code, body) = response(connection)
        if (code !in 200..299) throw IllegalStateException(errorMessage(body, "Session release failed"))
    }

    internal fun parseEnrollment(body: String): EnrollmentResult {
        val json = JSONObject(body)
        val node = json.getJSONObject("node")
        return EnrollmentResult(
            token = json.getString("session_token").also { require(it.isNotBlank()) { "Session token is missing" } },
            node = node.getString("display_name"),
            country = node.getJSONObject("location").getString("country_name"),
            transport = HysteriaTransport.select(json.getJSONArray("transports")),
        )
    }

    private fun normalizeBaseUrl(serverUrl: String): String {
        val url = URL(serverUrl.trim().trimEnd('/'))
        val localHttp = url.protocol == "http" && (url.host == "localhost" || url.host == "127.0.0.1" || url.host == "::1")
        require(url.protocol == "https" || localHttp) { "Use HTTPS for a remote server" }
        require(url.userInfo == null && url.query == null && url.ref == null && url.path.isBlank()) { "Server URL must not contain credentials, a path, query, or fragment" }
        return url.toExternalForm().trimEnd('/')
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
}
