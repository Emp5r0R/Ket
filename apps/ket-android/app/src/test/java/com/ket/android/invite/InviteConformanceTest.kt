package com.ket.android.invite

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/** Runs the shared Rust-generated vectors (`crates/ket-invite/tests/vectors/invite-v1.json`). */
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
            val fromText = InviteCapsule.decodeText(case.getString("text"), now)
            val fromBinary = InviteCapsule.decodeBinary(hex(case.getString("binary_hex")), now)
            for (capsule in listOf(fromText, fromBinary)) {
                assertEquals(name, case.getString("node_public_key_hex"), hex(capsule.nodePublicKey.encoded))
                assertEquals(name, case.getString("node_fingerprint"), capsule.nodePublicKey.fingerprint)
                assertEquals(name, case.getString("access_code"), capsule.accessCode)
                assertEquals(name, case.getInt("control_api_min"), capsule.controlApiMin)
                assertEquals(name, case.getInt("control_api_max"), capsule.controlApiMax)
                assertTrue(name, capsule.supportsControlApi(CONTROL_API_VERSION))
                val expiry = case.opt("access_expires_at_epoch_seconds").takeUnless { it == JSONObject.NULL }
                assertEquals(name, (expiry as Number?)?.toLong(), capsule.accessExpiresAtEpochSeconds)
                val expectedLocators = case.getJSONArray("locators").objects()
                assertEquals(name, expectedLocators.size, capsule.locators.size)
                expectedLocators.zip(capsule.locators).forEach { (expected, actual) ->
                    assertEquals(name, expected.getString("base_url"), actual.baseUrl)
                    assertEquals(name, expected.getInt("port"), actual.port)
                    when (actual) {
                        is ControlLocator.Https -> {
                            assertEquals(name, "https", expected.getString("type"))
                            assertEquals(name, expected.getString("host"), actual.host)
                        }
                        is ControlLocator.LoopbackDevelopment -> {
                            assertEquals(name, "loopback_development", expected.getString("type"))
                            assertEquals(name, expected.getString("host"), actual.host.address)
                        }
                    }
                }
                assertFalse(name, capsule.toString().contains(capsule.accessCode))
            }
            assertEquals(name, fromText.toString(), fromBinary.toString())
            val bare = case.getString("text").removePrefix(INVITE_TEXT_PREFIX)
            assertEquals(name, fromText.toString(), InviteCapsule.decodeText(bare, now).toString())
        }
    }

    @Test
    fun `invalid vectors fail with the reference error code`() {
        val cases = vectors.getJSONArray("invalid").objects()
        assertTrue(cases.size >= 15)
        for (case in cases) {
            val now = case.opt("now_epoch_seconds").takeUnless { it == JSONObject.NULL } as Number?
            val error = assertThrows(case.getString("name"), InviteException::class.java) {
                InviteCapsule.decodeText(case.getString("text"), now?.toLong())
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
    fun `canonical CBOR rejects non-shortest and unsupported encodings`() {
        val rejected = listOf(
            byteArrayOf(0x18, 0x17),
            byteArrayOf(0x19, 0x00, 0xff.toByte()),
            byteArrayOf(0x1a, 0x00, 0x00, 0xff.toByte(), 0xff.toByte()),
            byteArrayOf(0x1c),
            byteArrayOf(0x1f),
            byteArrayOf(0x20),
            byteArrayOf(0x19, 0x01),
        )
        for (input in rejected) {
            assertThrows(hex(input), InviteException::class.java) { CborDecoder(input).uint() }
        }
        val maximum = CborEncoder().apply { uint(ULong.MAX_VALUE) }.toByteArray()
        assertEquals(ULong.MAX_VALUE, CborDecoder(maximum).uint())
        assertThrows(InviteException::class.java) { CborDecoder(byteArrayOf(0x62, 0xc3.toByte(), 0x28)).text(8) }
    }

    @Test
    fun `public DNS names follow the reference locator rules`() {
        val accepted = listOf("ket.example.com", "ket-1-2-3-4.sslip.io", "a.b.co")
        val rejected = listOf(
            "Ket.example.com", "ket.example.com.", "-ket.example.com", "ket..example.com", "10.0.0.1",
            "single", "printer.local", "router.home.arpa", "node.test", "svc.internal", "x.onion",
            "${"a".repeat(64)}.com",
        )
        accepted.forEach { assertTrue(it, ControlLocator.isPublicDnsName(it)) }
        rejected.forEach { assertFalse(it, ControlLocator.isPublicDnsName(it)) }
    }

    private fun JSONArray.objects(): List<JSONObject> = List(length()) { getJSONObject(it) }

    private fun hex(value: String): ByteArray = value.chunked(2).map { it.toInt(16).toByte() }.toByteArray()

    private fun hex(value: ByteArray): String = value.joinToString("") { "%02x".format(it) }
}
