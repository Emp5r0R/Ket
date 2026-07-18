package com.ket.android

import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

data class EnrollmentResult(val token: String, val node: String, val country: String)
data class SessionTelemetry(val node: String, val sent: Long, val received: Long, val online: Int, val capacity: Double)

/** Small platform adapter for the versioned Ket control contract. Secrets never enter logs. */
object KetControlApi {
    fun enroll(serverUrl: String, accessCode: String, clientName: String): EnrollmentResult {
        val base = serverUrl.trimEnd('/')
        require(base.startsWith("https://") || base.startsWith("http://localhost") || base.startsWith("http://127.0.0.1")) { "Use HTTPS for a remote server" }
        require(accessCode.length == 32) { "Access code must be 32 characters" }
        val connection = (URL("$base/v1/sessions").openConnection() as HttpURLConnection).apply {
            requestMethod = "POST"; connectTimeout = 5_000; readTimeout = 15_000; doOutput = true
            setRequestProperty("Content-Type", "application/json")
        }
        connection.outputStream.use { it.write(JSONObject().put("access_code", accessCode).put("client_name", clientName).toString().toByteArray()) }
        val body = (if (connection.responseCode in 200..299) connection.inputStream else connection.errorStream).bufferedReader().use { it.readText() }
        if (connection.responseCode !in 200..299) throw IllegalStateException(JSONObject(body).optString("message", "Enrollment failed"))
        val json = JSONObject(body); val node = json.getJSONObject("node")
        return EnrollmentResult(json.getString("session_token"), node.getString("display_name"), node.getJSONObject("location").getString("country_name"))
    }

    fun status(serverUrl: String, token: String): SessionTelemetry {
        val connection = (URL("${serverUrl.trimEnd('/')}/v1/sessions/current").openConnection() as HttpURLConnection).apply {
            requestMethod = "GET"; connectTimeout = 5_000; readTimeout = 10_000; setRequestProperty("Authorization", "Bearer $token")
        }
        val body = connection.inputStream.bufferedReader().use { it.readText() }
        if (connection.responseCode !in 200..299) throw IllegalStateException("Session status unavailable")
        val json = JSONObject(body); val node = json.getJSONObject("node"); val traffic = json.getJSONObject("traffic")
        return SessionTelemetry(node.getString("display_name"), traffic.getLong("bytes_sent"), traffic.getLong("bytes_received"), traffic.getInt("online_connections"), node.getDouble("capacity_percent"))
    }

    fun renew(serverUrl: String, token: String): Long {
        val connection = (URL("${serverUrl.trimEnd('/')}/v1/sessions/current").openConnection() as HttpURLConnection).apply {
            requestMethod = "PUT"; connectTimeout = 5_000; readTimeout = 10_000; setRequestProperty("Authorization", "Bearer $token")
        }
        val code = connection.responseCode
        if (code !in 200..299) throw IllegalStateException("Session renewal failed")
        val expires = JSONObject(connection.inputStream.bufferedReader().use { it.readText() }).getLong("expires_at_epoch_seconds")
        connection.disconnect(); return expires
    }

    fun release(serverUrl: String, token: String) {
        val connection = (URL("${serverUrl.trimEnd('/')}/v1/sessions/current").openConnection() as HttpURLConnection).apply {
            requestMethod = "DELETE"; connectTimeout = 5_000; readTimeout = 10_000; setRequestProperty("Authorization", "Bearer $token")
        }
        val code = connection.responseCode
        if (code !in 200..299 && code != 204) throw IllegalStateException("Session release failed")
        connection.disconnect()
    }
}
