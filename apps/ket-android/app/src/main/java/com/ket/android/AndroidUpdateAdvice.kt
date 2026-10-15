package com.ket.android

import org.json.JSONObject
import java.net.URI

/** Request header naming this app to the server, which rates it against its releases. */
internal const val CLIENT_HEADER = "Ket-Client"
internal const val CLIENT_UPDATE_REQUIRED_CODE = "client_update_required"

/** How strongly the server asks this app to update, least to most urgent. */
enum class ClientUpdateStatus(val wireName: String) {
    Current("current"),
    Optional("optional"),
    Recommended("recommended"),
    Security("security"),
    Required("required");

    companion object {
        fun parse(value: String): ClientUpdateStatus? = entries.firstOrNull { it.wireName == value }
    }
}

data class AndroidClientUpdate(
    val status: ClientUpdateStatus,
    val latestVersion: String,
    val requiredAfterEpochSeconds: Long?,
    val summary: String?,
    /** Always an https:// page; anything else is dropped by the parser. */
    val downloadUrl: String,
)

data class AndroidUpdateAdvice(
    val serverVersion: String,
    val client: AndroidClientUpdate,
)

/**
 * Reads the server's update advice. Advice is informational, so anything
 * malformed is ignored instead of failing the session.
 */
internal object AndroidUpdateAdviceParser {
    private const val MAX_TEXT_CHARS = 256
    private const val MAX_VERSION_CHARS = 32
    private const val MAX_URL_CHARS = 2_048

    /** This app's header value, for example `android/1.2.0`. */
    fun clientHeaderValue(versionName: String = BuildConfig.VERSION_NAME): String = "android/$versionName"

    fun parseAdvice(json: JSONObject?): AndroidUpdateAdvice? {
        if (json == null) return null
        val serverVersion = boundedText(json.optString("server_version"), MAX_VERSION_CHARS) ?: return null
        val client = parseClientUpdate(json.optJSONObject("client")) ?: return null
        return AndroidUpdateAdvice(serverVersion, client)
    }

    fun parseClientUpdate(json: JSONObject?): AndroidClientUpdate? {
        if (json == null) return null
        val status = ClientUpdateStatus.parse(json.optString("status")) ?: return null
        val latestVersion = boundedText(json.optString("latest_version"), MAX_VERSION_CHARS) ?: return null
        val downloadUrl = safeDownloadUrl(json.optString("download_url")) ?: return null
        val requiredAfter = if (json.has("required_after_epoch_seconds") && !json.isNull("required_after_epoch_seconds")) {
            json.optLong("required_after_epoch_seconds", -1).takeIf { it > 0 } ?: return null
        } else {
            null
        }
        val summary = if (json.has("summary") && !json.isNull("summary")) {
            boundedText(json.optString("summary"), MAX_TEXT_CHARS)
        } else {
            null
        }
        return AndroidClientUpdate(status, latestVersion, requiredAfter, summary, downloadUrl)
    }

    /** The update details in a `426` body, when the server refused this app's version. */
    fun parseRefusal(code: Int, body: String): AndroidClientUpdate? {
        if (code != 426 || body.isBlank()) return null
        val json = runCatching { JSONObject(body) }.getOrNull() ?: return null
        if (json.optString("code") != CLIENT_UPDATE_REQUIRED_CODE) return null
        return parseClientUpdate(json.optJSONObject("update"))
    }

    private fun boundedText(value: String, maximum: Int): String? =
        value.filterNot(Char::isISOControl).trim().take(maximum).ifBlank { null }

    private fun safeDownloadUrl(value: String): String? {
        if (value.isBlank() || value.length > MAX_URL_CHARS) return null
        val uri = runCatching { URI(value) }.getOrNull() ?: return null
        return value.takeIf {
            uri.scheme == "https" && !uri.host.isNullOrBlank() && uri.rawUserInfo == null
        }
    }
}
