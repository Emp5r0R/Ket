package com.ket.android

import java.io.ByteArrayInputStream
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class KetControlApiTest {
    @Test
    fun `authorization status codes are terminal`() {
        assertTrue(KetControlException(401, "unauthorized").authorizationLost)
        assertTrue(KetControlException(403, "forbidden").authorizationLost)
        assertFalse(KetControlException(500, "server error").authorizationLost)
    }

    @Test
    fun `enrollment requires a shaped token live lease and bounded client identity`() {
        val token = testSessionToken('A')
        val enrollment = KetControlApi.parseEnrollment(
            testSessionManifest(token),
            requireActiveLease = true,
        )

        assertEquals(token, enrollment.token)
        assertEquals(FUTURE_EPOCH_SECONDS, enrollment.expiresAtEpochSeconds)
        assertEquals(ACCESS_EXPIRY_EPOCH_SECONDS, enrollment.accessExpiresAtEpochSeconds)
        assertEquals("Test node", enrollment.node.displayName)
        assertEquals("hy2-primary", enrollment.transports.single().id)

        assertThrows(IllegalArgumentException::class.java) {
            KetControlApi.parseEnrollment(testSessionManifest("short"))
        }
        assertThrows(IllegalArgumentException::class.java) {
            KetControlApi.parseEnrollment(
                JSONObject(testSessionManifest(token))
                    .put("session_expires_at_epoch_seconds", 1)
                    .toString(),
                requireActiveLease = true,
            )
        }
        assertThrows(IllegalArgumentException::class.java) {
            KetControlApi.validateClientName(" device ")
        }
    }

    @Test
    fun `session status is bound to the token and carries strict telemetry`() {
        val token = testSessionToken('B')
        val telemetry = KetControlApi.parseStatus(
            testSessionStatus(token.take(12)).toString(),
            expectedSessionId = token.take(12),
        )

        assertEquals(FUTURE_EPOCH_SECONDS, telemetry.expiresAtEpochSeconds)
        assertEquals(ACCESS_EXPIRY_EPOCH_SECONDS, telemetry.accessExpiresAtEpochSeconds)
        assertEquals("Test node", telemetry.node.displayName)
        assertTrue(telemetry.available)
        assertEquals(1_024L, telemetry.sent)
        assertEquals(8_192L, telemetry.received)
        assertEquals(2, telemetry.online)
        assertEquals(FUTURE_EPOCH_SECONDS, telemetry.observedAtEpochSeconds)
    }

    @Test
    fun `session status rejects identity device and lease mismatches`() {
        val token = testSessionToken('C')
        val sessionId = token.take(12)

        assertThrows(IllegalArgumentException::class.java) {
            KetControlApi.parseStatus(
                testSessionStatus(testSessionToken('D').take(12)).toString(),
                expectedSessionId = sessionId,
            )
        }
        assertThrows(IllegalArgumentException::class.java) {
            KetControlApi.parseStatus(
                testSessionStatus(sessionId).put("client_name", "Other client").toString(),
                expectedSessionId = sessionId,
            )
        }
        assertThrows(IllegalArgumentException::class.java) {
            KetControlApi.parseStatus(
                testSessionStatus(sessionId).put("expires_at_epoch_seconds", 1).toString(),
                expectedSessionId = sessionId,
            )
        }
    }

    @Test
    fun `session status rejects invalid or inconsistent traffic telemetry`() {
        val sessionId = testSessionToken('E').take(12)
        val negative = testSessionStatus(sessionId).apply {
            getJSONObject("traffic").put("bytes_sent", -1)
        }
        val unavailableWithCounters = testSessionStatus(sessionId).apply {
            getJSONObject("traffic").put("available", false)
        }
        val missingObservation = testSessionStatus(sessionId).apply {
            getJSONObject("traffic").put("observed_at_epoch_seconds", 0)
        }

        listOf(negative, unavailableWithCounters, missingObservation).forEach { body ->
            assertThrows(IllegalArgumentException::class.java) {
                KetControlApi.parseStatus(body.toString(), expectedSessionId = sessionId)
            }
        }
    }

    @Test
    fun `response reader caps declared and streamed bytes and requires UTF8`() {
        val body = "{\"ok\":true}".toByteArray()
        assertEquals(
            "{\"ok\":true}",
            KetControlApi.readBoundedBody(ByteArrayInputStream(body), body.size.toLong()),
        )
        assertThrows(IllegalArgumentException::class.java) {
            KetControlApi.readBoundedBody(ByteArrayInputStream(byteArrayOf()), 128L * 1_024 + 1)
        }
        assertThrows(IllegalArgumentException::class.java) {
            KetControlApi.readBoundedBody(
                ByteArrayInputStream(ByteArray(128 * 1_024 + 1)),
                declaredLength = -1,
            )
        }
        assertThrows(IllegalArgumentException::class.java) {
            KetControlApi.readBoundedBody(
                ByteArrayInputStream(byteArrayOf(0xc3.toByte(), 0x28)),
                declaredLength = 2,
            )
        }
    }

    @Test
    fun `server error text is control free and bounded`() {
        val sanitized = KetControlApi.sanitizeServerMessage("failure\n" + "x".repeat(300))

        assertFalse(sanitized.contains('\n'))
        assertEquals(256, sanitized.length)
    }
}

internal fun testSessionToken(seed: Char): String = seed.toString().repeat(12) + "S".repeat(32)

internal fun testSessionManifest(token: String): String = JSONObject()
    .put("session_token", token)
    .put("session_expires_at_epoch_seconds", FUTURE_EPOCH_SECONDS)
    .put("access_expires_at_epoch_seconds", ACCESS_EXPIRY_EPOCH_SECONDS)
    .put("node", testNodeJson())
    .put(
        "transports",
        JSONArray().put(
            JSONObject()
                .put("id", "hy2-primary")
                .put("protocol", "hysteria2")
                .put("endpoint", "vpn.example.test")
                .put("port", 443)
                .put("network", "udp")
                .put("priority", 10)
                .put("tls_server_name", "vpn.example.test")
                .put("options", JSONObject().put("obfs", "none"))
                .put(
                    "credential",
                    JSONObject()
                        .put("auth", "transport-auth-secret")
                        .put("secrets", JSONObject()),
                ),
        ),
    )
    .toString()

private fun testSessionStatus(sessionId: String): JSONObject = JSONObject()
    .put("session_id", sessionId)
    .put("client_name", KET_ANDROID_CLIENT_NAME)
    .put("expires_at_epoch_seconds", FUTURE_EPOCH_SECONDS)
    .put("access_expires_at_epoch_seconds", ACCESS_EXPIRY_EPOCH_SECONDS)
    .put("node", testNodeJson())
    .put(
        "traffic",
        JSONObject()
            .put("available", true)
            .put("bytes_sent", 1_024)
            .put("bytes_received", 8_192)
            .put("online_connections", 2)
            .put("observed_at_epoch_seconds", FUTURE_EPOCH_SECONDS),
    )

private const val FUTURE_EPOCH_SECONDS = 4_000_000_000L
private const val ACCESS_EXPIRY_EPOCH_SECONDS = FUTURE_EPOCH_SECONDS + 3_600L
