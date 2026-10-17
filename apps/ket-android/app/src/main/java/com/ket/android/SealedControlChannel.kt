package com.ket.android

import com.ket.android.invite.InviteException
import com.ket.android.invite.InviteFailure
import com.ket.android.invite.NativeInvites
import com.ket.android.invite.NodePublicKey
import org.json.JSONObject
import java.nio.charset.StandardCharsets
import java.util.concurrent.atomic.AtomicLong

/** The raw outer answer to a sealed request: its HTTP status and body bytes. */
internal class SealedHttpResponse(val code: Int, val body: ByteArray)

/** The session API's answer carried inside a sealed response. */
internal class SealedAnswer(val code: Int, val body: String)

/**
 * Session API requests sealed to the pinned node key (`POST /v1/sealed`, crates/ket-seal). A front
 * that terminates TLS sees one route and opaque bodies: not the access code, the session token,
 * the credentials the node returns, the app version, or whether a request succeeded. Only the
 * pinned node can open a request and only this app can open its answer, so an answer also proves
 * the node's identity.
 */
internal class SealedControlChannel(
    /** Posts the sealed bytes to the URL and returns the bounded answer. */
    private val post: (url: String, body: ByteArray) -> SealedHttpResponse,
    private val clientHeader: () -> String,
    private val nowEpochSeconds: () -> Long = { System.currentTimeMillis() / 1_000 },
) {
    private val clockOffsetSeconds = AtomicLong(0)

    /** Null when the server has no sealed route (an older server). */
    fun exchange(
        baseUrl: String,
        node: NodePublicKey,
        method: String,
        path: String,
        bearer: String? = null,
        body: String? = null,
    ): SealedAnswer? {
        var answer = attempt(baseUrl, node, method, path, bearer, body) ?: return null
        if (answer.isClockSkew()) {
            clockOffsetSeconds.set(answer.serverTime - nowEpochSeconds())
            answer = attempt(baseUrl, node, method, path, bearer, body) ?: return null
        }
        return SealedAnswer(answer.status, answer.body)
    }

    private class Inner(val status: Int, val serverTime: Long, val body: String) {
        fun isClockSkew(): Boolean = status == 400 &&
            runCatching { JSONObject(body).optString("code") == CLOCK_SKEW_CODE }.getOrDefault(false)
    }

    private fun attempt(
        baseUrl: String,
        node: NodePublicKey,
        method: String,
        path: String,
        bearer: String?,
        body: String?,
    ): Inner? {
        val request = JSONObject()
            .put("method", method)
            .put("path", path)
            .put("sent_at_epoch_seconds", nowEpochSeconds() + clockOffsetSeconds.get())
            .put("client", clientHeader())
        bearer?.let { request.put("bearer", it) }
        body?.let { request.put("body", it) }
        val plaintext = request.toString().toByteArray(StandardCharsets.UTF_8)
        val sealed = try {
            NativeInvites.sealRequest(node.base64Url, plaintext)
        } finally {
            plaintext.fill(0)
        } ?: throw InviteException(InviteFailure.InvalidNodeKey)
        val (sealedRequest, opener) = sealed[0] to sealed[1]
        try {
            val response = post("$baseUrl$PATH", sealedRequest)
            when (response.code) {
                200 -> Unit
                404 -> return null
                else -> throw KetControlException(response.code, "The server could not take the request")
            }
            val answer = NativeInvites.openResponse(opener, response.body)
                ?: throw InviteException(InviteFailure.NodeIdentityMismatch)
            try {
                val json = JSONObject(String(answer, StandardCharsets.UTF_8))
                return Inner(
                    status = json.getInt("status"),
                    serverTime = json.getLong("server_time_epoch_seconds"),
                    body = if (json.has("body") && !json.isNull("body")) json.getString("body") else "",
                )
            } finally {
                answer.fill(0)
            }
        } finally {
            opener.fill(0)
        }
    }

    companion object {
        const val PATH = "/v1/sealed"
        const val CONTENT_TYPE = "application/octet-stream"
        const val MAX_SEALED_BYTES = 256 * 1024
        private const val CLOCK_SKEW_CODE = "request_clock_skew"
    }
}
