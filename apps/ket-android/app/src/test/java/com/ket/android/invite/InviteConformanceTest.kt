package com.ket.android.invite

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Base64

/**
 * Runs the shared Rust-generated vectors (`crates/ket-invite/tests/vectors/invite-v1.json`) through
 * the app's JNI bridge to the Rust implementation.
 */
class InviteConformanceTest {
    private val vectors: JSONObject = javaClass.classLoader!!
        .getResourceAsStream("invite-v1.json")!!
        .use { JSONObject(it.readBytes().toString(Charsets.UTF_8)) }

    @Test
    fun `valid vectors decode to the same fields as the reference implementation`() {
        val cases = vectors.getJSONArray("valid").objects()
        assertTrue(cases.isNotEmpty())
        for (case in cases) {
            val name = case.getString("name")
            val now = case.getLong("decode_at_epoch_seconds")
            val bare = case.getString("text").removePrefix(INVITE_TEXT_PREFIX)
            for (text in listOf(case.getString("text"), bare)) {
                val invite = NativeInvite.read(text, now)
                assertEquals(name, base64Url(hex(case.getString("node_public_key_hex"))), invite.nodeKey.base64Url)
                assertEquals(name, case.getString("node_fingerprint"), invite.nodeKey.fingerprint)
                assertEquals(name, case.getString("access_code"), invite.accessCode)
                val expiry = case.opt("access_expires_at_epoch_seconds").takeUnless { it == JSONObject.NULL }
                assertEquals(name, (expiry as Number?)?.toLong(), invite.accessExpiresAtEpochSeconds)
                val expectedOrigins = case.getJSONArray("locators").objects().map { it.getString("base_url").trimEnd('/') }
                assertEquals(name, expectedOrigins, invite.origins)
                assertFalse(name, invite.toString().contains(invite.accessCode))
            }
        }
    }

    @Test
    fun `invalid vectors fail with the reference error code`() {
        val cases = vectors.getJSONArray("invalid").objects()
        assertTrue(cases.size >= 15)
        for (case in cases) {
            val now = case.opt("now_epoch_seconds").takeUnless { it == JSONObject.NULL } as Number?
            val error = assertThrows(case.getString("name"), InviteException::class.java) {
                // Rust requires a clock; the vectors without one are rejected for other reasons.
                NativeInvite.read(case.getString("text"), now?.toLong() ?: 0)
            }
            assertEquals(case.getString("name"), case.getString("error"), error.failure.code)
            assertFalse(case.getString("name"), error.message.orEmpty().contains(case.getString("text")))
        }
    }

    @Test
    fun `enrollment signatures verify only for the pinned node nonce and exact body`() {
        val enrollment = vectors.getJSONObject("enrollment")
        val nodeKey = NodePublicKey.fromBytes(hex(enrollment.getString("node_public_key_hex")))
        EnrollmentProof.verify(
            nodeKey,
            EnrollmentNonce.fromHeaderValue(enrollment.getString("nonce")),
            enrollment.getString("body").toByteArray(Charsets.UTF_8),
            enrollment.getString("signature"),
        )
        val rejected = enrollment.getJSONArray("rejected").objects()
        assertEquals(5, rejected.size)
        for (case in rejected) {
            val signature = case.opt("signature").takeUnless { it == JSONObject.NULL } as String?
            val error = assertThrows(case.getString("name"), InviteException::class.java) {
                EnrollmentProof.verify(
                    nodeKey,
                    EnrollmentNonce.fromHeaderValue(case.getString("nonce")),
                    case.getString("body").toByteArray(Charsets.UTF_8),
                    signature,
                )
            }
            assertEquals(case.getString("name"), InviteFailure.NodeIdentityMismatch, error.failure)
        }
    }

    @Test
    fun `nonces are fresh canonical 32-byte values`() {
        val nonce = EnrollmentNonce.generate()
        assertEquals(43, nonce.headerValue.length)
        assertEquals(nonce, EnrollmentNonce.fromHeaderValue(nonce.headerValue))
        assertNotEquals(nonce, EnrollmentNonce.generate())
        assertEquals("EnrollmentNonce(..)", nonce.toString())
        for (invalid in listOf("${nonce.headerValue}=", nonce.headerValue.drop(1), "${nonce.headerValue}AA", "", "a+b/")) {
            assertThrows(invalid, InviteException::class.java) { EnrollmentNonce.fromHeaderValue(invalid) }
        }
    }

    @Test
    fun `stored node keys are validated by the reference implementation`() {
        val enrollment = vectors.getJSONObject("enrollment")
        val key = NodePublicKey.fromBytes(hex(enrollment.getString("node_public_key_hex")))
        assertEquals(key, NodePublicKey.fromBase64Url(key.base64Url))
        assertEquals("NodePublicKey(${key.fingerprint})", key.toString())
        for (invalid in listOf("${key.base64Url}=", key.base64Url.drop(1), base64Url(ByteArray(32)), "bad key", "")) {
            val error = assertThrows(invalid, InviteException::class.java) { NodePublicKey.fromBase64Url(invalid) }
            assertEquals(invalid, InviteFailure.InvalidNodeKey, error.failure)
        }
    }

    @Test
    fun `every reference error code maps to its own failure`() {
        for (failure in InviteFailure.entries) {
            assertEquals(failure, InviteFailure.fromCode(failure.code))
        }
        assertEquals(InviteFailure.Invalid, InviteFailure.fromCode("invalid_invite_checksum"))
    }

    private fun JSONArray.objects(): List<JSONObject> = List(length()) { getJSONObject(it) }

    private fun hex(value: String): ByteArray = value.chunked(2).map { it.toInt(16).toByte() }.toByteArray()

    private fun base64Url(value: ByteArray): String = Base64.getUrlEncoder().withoutPadding().encodeToString(value)
}
