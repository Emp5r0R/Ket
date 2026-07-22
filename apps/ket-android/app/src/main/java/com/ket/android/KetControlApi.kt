package com.ket.android

import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.net.HttpURLConnection
import java.net.URL
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction
import java.nio.charset.StandardCharsets

internal const val KET_ANDROID_CLIENT_NAME = "Ket Android"

class EnrollmentResult(
    val token: String,
    val expiresAtEpochSeconds: Long,
    val accessExpiresAtEpochSeconds: Long?,
    val node: AndroidNodeStatus,
    val transports: List<AndroidTransport>,
    internal val manifestJson: String,
) {
    override fun toString(): String =
        "EnrollmentResult(token=[REDACTED], node=${node.displayName}, transports=$transports)"
}
data class SessionTelemetry(
    val expiresAtEpochSeconds: Long,
    val accessExpiresAtEpochSeconds: Long?,
    val node: AndroidNodeStatus,
    val available: Boolean,
    val sent: Long,
    val received: Long,
    val online: Int,
    val observedAtEpochSeconds: Long,
)

internal class KetControlException(
    val statusCode: Int,
    message: String,
) : IllegalStateException(message) {
    val authorizationLost: Boolean = statusCode == 401 || statusCode == 403
}

/** Small platform adapter for the versioned Ket control contract. Secrets never enter logs. */
object KetControlApi : TunnelSessionApi {
    private const val SESSION_TOKEN_LENGTH = 44
    private const val SESSION_ID_LENGTH = 12
    private const val MAX_RESPONSE_BYTES = 128 * 1024
    private const val MAX_ERROR_MESSAGE_CHARS = 256

    override fun enroll(serverUrl: String, accessCode: String, clientName: String): EnrollmentResult {
        val base = normalizeBaseUrl(serverUrl)
        val validatedAccessCode = validateAccessCode(accessCode)
        val validatedClientName = validateClientName(clientName)
        val connection = open("$base/v1/sessions").apply {
            requestMethod = "POST"; readTimeout = 15_000; doOutput = true
            setRequestProperty("Content-Type", "application/json")
        }
        connection.outputStream.use {
            it.write(
                JSONObject()
                    .put("access_code", validatedAccessCode)
                    .put("client_name", validatedClientName)
                    .toString()
                    .toByteArray(StandardCharsets.UTF_8),
            )
        }
        val (code, body) = response(connection)
        requireSuccess(code, HttpURLConnection.HTTP_CREATED, body, "Enrollment failed")
        return parseEnrollment(body, requireActiveLease = true)
    }

    fun status(serverUrl: String, token: String): SessionTelemetry {
        val validatedToken = validateSessionToken(token)
        val connection = open("${normalizeBaseUrl(serverUrl)}/v1/sessions/current").apply {
            requestMethod = "GET"; setRequestProperty("Authorization", "Bearer $validatedToken")
        }
        val (code, body) = response(connection)
        requireSuccess(code, HttpURLConnection.HTTP_OK, body, "Session status unavailable")
        return parseStatus(body, validatedToken.take(SESSION_ID_LENGTH))
    }

    override fun renew(serverUrl: String, token: String): Long {
        val validatedToken = validateSessionToken(token)
        val connection = open("${normalizeBaseUrl(serverUrl)}/v1/sessions/current").apply {
            requestMethod = "PUT"; setRequestProperty("Authorization", "Bearer $validatedToken")
        }
        val (code, body) = response(connection)
        requireSuccess(code, HttpURLConnection.HTTP_OK, body, "Session renewal failed")
        return parseStatus(body, validatedToken.take(SESSION_ID_LENGTH)).expiresAtEpochSeconds
    }

    override fun release(serverUrl: String, token: String) {
        val validatedToken = validateSessionToken(token)
        val connection = open("${normalizeBaseUrl(serverUrl)}/v1/sessions/current").apply {
            requestMethod = "DELETE"; setRequestProperty("Authorization", "Bearer $validatedToken")
        }
        val (code, body) = response(connection)
        requireSuccess(code, HttpURLConnection.HTTP_NO_CONTENT, body, "Session release failed")
    }

    internal fun parseEnrollment(
        body: String,
        requireActiveLease: Boolean = false,
    ): EnrollmentResult {
        val json = JSONObject(body)
        val expiresAt = positiveLong(json, "session_expires_at_epoch_seconds").also {
            if (requireActiveLease) require(it > epochSeconds()) { "Session lease is already expired" }
        }
        val accessExpiresAt = optionalPositiveLong(json, "access_expires_at_epoch_seconds").also {
            if (requireActiveLease && it != null) require(it > epochSeconds()) { "Access time has expired" }
            require(it == null || expiresAt <= it) { "Session lease outlives access time" }
        }
        return EnrollmentResult(
            token = validateSessionToken(json.getString("session_token")),
            expiresAtEpochSeconds = expiresAt,
            accessExpiresAtEpochSeconds = accessExpiresAt,
            node = AndroidNodeStatusParser.parse(json.getJSONObject("node")),
            transports = AndroidTransportSelector.parse(json.getJSONArray("transports")),
            manifestJson = json.toString(),
        )
    }

    internal fun parseStatus(
        body: String,
        expectedSessionId: String,
        expectedClientName: String = KET_ANDROID_CLIENT_NAME,
    ): SessionTelemetry {
        val json = JSONObject(body)
        val sessionId = validateSessionId(json.getString("session_id"))
        require(sessionId == validateSessionId(expectedSessionId)) {
            "Session status identity does not match enrollment"
        }
        val clientName = validateClientName(json.getString("client_name"))
        require(clientName == validateClientName(expectedClientName)) {
            "Session status client name does not match enrollment"
        }
        val expiresAt = positiveLong(json, "expires_at_epoch_seconds").also {
            require(it > epochSeconds()) { "Session lease is already expired" }
        }
        val accessExpiresAt = optionalPositiveLong(json, "access_expires_at_epoch_seconds").also {
            require(it == null || it > epochSeconds()) { "Access time has expired" }
            require(it == null || expiresAt <= it) { "Session lease outlives access time" }
        }
        val traffic = json.getJSONObject("traffic")
        val available = traffic.getBoolean("available")
        val sent = nonNegativeLong(traffic, "bytes_sent")
        val received = nonNegativeLong(traffic, "bytes_received")
        val online = traffic.getInt("online_connections").also {
            require(it >= 0) { "Online connection count is invalid" }
        }
        if (!available) {
            require(sent == 0L && received == 0L && online == 0) {
                "Unavailable traffic telemetry is inconsistent"
            }
        }
        return SessionTelemetry(
            expiresAtEpochSeconds = expiresAt,
            accessExpiresAtEpochSeconds = accessExpiresAt,
            node = AndroidNodeStatusParser.parse(json.getJSONObject("node")),
            available = available,
            sent = sent,
            received = received,
            online = online,
            observedAtEpochSeconds = positiveLong(traffic, "observed_at_epoch_seconds"),
        )
    }

    private fun optionalPositiveLong(json: JSONObject, key: String): Long? =
        if (json.has(key) && !json.isNull(key)) positiveLong(json, key) else null

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

    internal fun validateSessionToken(token: String): String = token.also {
        require(
            it.length == SESSION_TOKEN_LENGTH &&
                it.all { character -> character.isLetterOrDigit() && character.code <= 127 },
        ) { "Session token has an invalid shape" }
    }

    internal fun validateClientName(clientName: String): String = boundedText(
        clientName,
        "Client name",
        96,
    )

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
            code to readBoundedBody(stream, connection.contentLengthLong)
        } finally {
            connection.disconnect()
        }
    }

    private fun errorMessage(body: String, fallback: String): String {
        if (body.isBlank()) return fallback
        return sanitizeServerMessage(
            runCatching { JSONObject(body).optString("message", fallback) }.getOrDefault(fallback),
        ).ifBlank { fallback }
    }

    private fun requireSuccess(code: Int, expected: Int, body: String, fallback: String) {
        if (code != expected) throw KetControlException(code, errorMessage(body, fallback))
    }

    private fun nonNegativeLong(json: JSONObject, key: String): Long = json.getLong(key).also {
        require(it >= 0) { "$key is invalid" }
    }

    private fun positiveLong(json: JSONObject, key: String): Long = json.getLong(key).also {
        require(it > 0) { "$key is invalid" }
    }

    private fun validateSessionId(sessionId: String): String = sessionId.also {
        require(
            it.length == SESSION_ID_LENGTH &&
                it.all { character -> character.isLetterOrDigit() && character.code <= 127 },
        ) { "Session ID has an invalid shape" }
    }

    private fun boundedText(value: String, label: String, maximumLength: Int): String =
        value.also {
            require(
                it.isNotBlank() &&
                    it == it.trim() &&
                    it.length <= maximumLength &&
                    it.none(Char::isISOControl),
            ) { "$label is invalid" }
        }

    internal fun readBoundedBody(stream: InputStream?, declaredLength: Long = -1): String {
        require(declaredLength in -1..MAX_RESPONSE_BYTES.toLong()) {
            "Control response exceeded the size limit"
        }
        if (stream == null) return ""
        val output = ByteArrayOutputStream(
            when (declaredLength) {
                in 0..MAX_RESPONSE_BYTES.toLong() -> declaredLength.toInt()
                else -> 0
            },
        )
        stream.use { input ->
            val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
            while (true) {
                val count = input.read(buffer)
                if (count < 0) break
                require(output.size() + count <= MAX_RESPONSE_BYTES) {
                    "Control response exceeded the size limit"
                }
                output.write(buffer, 0, count)
            }
        }
        return try {
            StandardCharsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.REPORT)
                .onUnmappableCharacter(CodingErrorAction.REPORT)
                .decode(ByteBuffer.wrap(output.toByteArray()))
                .toString()
        } catch (_: Exception) {
            throw IllegalArgumentException("Control response is not valid UTF-8")
        }
    }

    internal fun sanitizeServerMessage(message: String): String = message
        .asSequence()
        .filterNot(Char::isISOControl)
        .take(MAX_ERROR_MESSAGE_CHARS)
        .joinToString("")

    private fun epochSeconds(): Long = System.currentTimeMillis() / 1_000
}
