package com.ket.android.invite

import com.ket.android.appInviteReader
import com.ket.android.inviteFailureMessage
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.assertThrows
import org.junit.Test

class InviteReaderTest {
    private val vectors: JSONObject = javaClass.classLoader!!
        .getResourceAsStream("invite-v1.json")!!
        .use { JSONObject(it.readBytes().toString(Charsets.UTF_8)) }

    private fun valid(name: String): JSONObject {
        val cases = vectors.getJSONArray("valid")
        return List(cases.length()) { cases.getJSONObject(it) }.first { it.getString("name") == name }
    }

    @Test
    fun `app reader yields the first reachable locator, code, expiry, and node key`() {
        val case = valid("single_https")
        val target = appInviteReader.read(" ${case.getString("text")}\n", case.getLong("decode_at_epoch_seconds"))
        assertEquals("https://ket.example.com", target.serverUrl)
        assertEquals(case.getString("access_code"), target.accessCode)
        assertEquals(case.getLong("access_expires_at_epoch_seconds"), target.accessExpiresAtEpochSeconds)
        assertEquals(case.getString("node_fingerprint"), target.nodeKey.fingerprint)
        assertTrue("current invites require sealed control", target.nodeKey.sealedControl)
        assertEquals(target.nodeKey, NodePublicKey.fromBase64Url(target.nodeKey.base64Url).withSealedControl(true))
        assertFalse(target.toString().contains(target.accessCode))

        val multiple = valid("multiple_locators_without_expiry")
        val first = appInviteReader.read(multiple.getString("text"), 1)
        assertEquals("https://edge.example.net:8443", first.serverUrl)
        assertEquals(null, first.accessExpiresAtEpochSeconds)
    }

    @Test
    fun `readers reject unreachable, expired, and malformed invites with safe messages`() {
        val case = valid("single_https")
        val unreachable = assertThrows(InviteException::class.java) {
            RustInviteReader { false }.read(case.getString("text"), 1)
        }
        assertEquals(InviteFailure.UnsupportedLocator, unreachable.failure)

        val expired = assertThrows(InviteException::class.java) {
            appInviteReader.read(case.getString("text"), case.getLong("access_expires_at_epoch_seconds"))
        }
        assertEquals(InviteFailure.Expired, expired.failure)
        assertEquals("This invite has expired. Ask for a new one.", inviteFailureMessage(expired))
        assertEquals("This invite could not be read.", inviteFailureMessage(IllegalStateException("secret")))

        for (invalid in listOf("", "AAAA=", "ket:invite:${"A".repeat(3_000)}")) {
            assertThrows(invalid.take(20), InviteException::class.java) { appInviteReader.read(invalid, 1) }
        }
        assertThrows(InviteException::class.java) { NodePublicKey.fromBase64Url("bad key") }
    }
}
