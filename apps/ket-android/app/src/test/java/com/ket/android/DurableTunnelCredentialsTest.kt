package com.ket.android

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import javax.crypto.AEADBadTagException
import javax.crypto.KeyGenerator

class DurableTunnelCredentialsTest {
    @Test
    fun `credential codec round trips a strict resumable session without exposing secrets`() {
        val original = credentials(sessionManifest("old-session-token"))

        val decoded = DurableTunnelCredentialsCodec.decode(
            DurableTunnelCredentialsCodec.encode(original),
        )
        val launch = requireNotNull(decoded.launchSpec())

        assertEquals(SERVER_URL, decoded.profile.serverUrl)
        assertEquals(ACCESS_CODE, decoded.profile.accessCode)
        assertEquals("old-session-token", launch.sessionToken)
        assertEquals("Hysteria 2", launch.transports.single().displayName)
        assertFalse(decoded.toString().contains(ACCESS_CODE))
        assertFalse(decoded.toString().contains("old-session-token"))
        assertFalse(decoded.toString().contains("transport-auth-secret"))
    }

    @Test
    fun `credential codec rejects unknown schemas fields and non ASCII access codes`() {
        val wrongSchema = JSONObject(String(DurableTunnelCredentialsCodec.encode(credentials(null))))
            .put("schema_version", 2)
            .toString()
            .toByteArray()
        val unknownField = JSONObject(String(DurableTunnelCredentialsCodec.encode(credentials(null))))
            .put("unexpected", true)
            .toString()
            .toByteArray()

        assertThrows(IllegalArgumentException::class.java) {
            DurableTunnelCredentialsCodec.decode(wrongSchema)
        }
        assertThrows(IllegalArgumentException::class.java) {
            DurableTunnelCredentialsCodec.decode(unknownField)
        }
        assertThrows(IllegalArgumentException::class.java) {
            KetControlApi.validateAccessCode("A".repeat(31) + "é")
        }
    }

    @Test
    fun `credential envelope hides plaintext and rejects authenticated tampering`() {
        val plaintext = DurableTunnelCredentialsCodec.encode(
            credentials(sessionManifest("sealed-session-token")),
        )
        val key = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
        val sealed = TunnelCredentialEnvelope.seal(plaintext, key)

        assertFalse(String(sealed).contains(ACCESS_CODE))
        assertFalse(String(sealed).contains("sealed-session-token"))
        assertTrue(plaintext.contentEquals(TunnelCredentialEnvelope.open(sealed, key)))

        sealed[sealed.lastIndex] = (sealed.last().toInt() xor 1).toByte()
        assertThrows(AEADBadTagException::class.java) {
            TunnelCredentialEnvelope.open(sealed, key)
        }
    }

    @Test
    fun `explicit app launch takes precedence without reading durable credentials`() {
        val pending = requireNotNull(credentials(sessionManifest("pending-token")).launchSpec())
        val store = FakeCredentialStore(null)
        val api = FakeSessionApi(enrollment("unused-token"))

        val resolved = DurableTunnelSessionResolver(api, store).resolve(pending)

        assertSame(pending, resolved)
        assertEquals(0, store.loads)
        assertEquals(0, api.renewals)
        assertEquals(0, api.enrollments)
    }

    @Test
    fun `process restart renews and resumes the existing server session`() {
        val store = FakeCredentialStore(credentials(sessionManifest("existing-token")))
        val api = FakeSessionApi(enrollment("unused-token"))

        val resolved = DurableTunnelSessionResolver(api, store).resolve(null)

        assertEquals("existing-token", resolved.sessionToken)
        assertEquals(1, api.renewals)
        assertEquals(0, api.enrollments)
        assertEquals(0, store.saves)
    }

    @Test
    fun `manual reconnect to the saved profile reuses its existing connection slot`() {
        val store = FakeCredentialStore(credentials(sessionManifest("existing-token")))
        val api = FakeSessionApi(enrollment("unused-token"))
        val profile = TunnelEnrollmentProfile(SERVER_URL, ACCESS_CODE)

        val resolved = DurableTunnelSessionResolver(api, store).resolveForApp(profile)

        assertEquals("existing-token", resolved.sessionToken)
        assertEquals(1, api.renewals)
        assertEquals(0, api.enrollments)
        assertEquals(0, api.releasedTokens.size)
    }

    @Test
    fun `blocked control endpoint retains the existing session for transport recovery`() {
        val store = FakeCredentialStore(credentials(sessionManifest("existing-token")))
        val api = FakeSessionApi(
            enrollment("unused-token"),
            renewalError = IllegalStateException("control endpoint blocked"),
        )

        val resolved = DurableTunnelSessionResolver(api, store).resolve(null)

        assertEquals("existing-token", resolved.sessionToken)
        assertEquals(0, api.enrollments)
        assertEquals(0, store.clears)
    }

    @Test
    fun `expired session is replaced from the encrypted enrollment profile`() {
        val store = FakeCredentialStore(credentials(sessionManifest("expired-token")))
        val api = FakeSessionApi(
            enrollment("replacement-token"),
            renewalError = KetControlException(401, "expired"),
        )

        val resolved = DurableTunnelSessionResolver(api, store).resolve(null)

        assertEquals("replacement-token", resolved.sessionToken)
        assertEquals(1, api.renewals)
        assertEquals(1, api.enrollments)
        assertEquals(1, store.clears)
        assertEquals("replacement-token", requireNotNull(store.current?.launchSpec()).sessionToken)
    }

    @Test
    fun `always on start creates a session when only enrollment is stored`() {
        val store = FakeCredentialStore(credentials(null))
        val api = FakeSessionApi(enrollment("fresh-token"))

        val resolved = DurableTunnelSessionResolver(api, store).resolve(null)

        assertEquals("fresh-token", resolved.sessionToken)
        assertEquals(1, api.enrollments)
        assertEquals(1, store.saves)
    }

    @Test
    fun `failed durable save releases the newly created server session`() {
        val store = FakeCredentialStore(credentials(null), failSaves = true)
        val api = FakeSessionApi(enrollment("orphan-token"))

        assertThrows(IllegalStateException::class.java) {
            DurableTunnelSessionResolver(api, store).resolve(null)
        }

        assertEquals(listOf("orphan-token"), api.releasedTokens)
    }

    @Test
    fun `unconfigured always on start fails before contacting the server`() {
        val store = FakeCredentialStore(null)
        val api = FakeSessionApi(enrollment("unused-token"))

        val error = assertThrows(IllegalStateException::class.java) {
            DurableTunnelSessionResolver(api, store).resolve(null)
        }

        assertTrue(error.message.orEmpty().contains("Connect once"))
        assertEquals(0, api.enrollments)
        assertNull(store.current)
    }

    private class FakeCredentialStore(
        var current: DurableTunnelCredentials?,
        private val failSaves: Boolean = false,
    ) : TunnelCredentialStore {
        var loads = 0
        var saves = 0
        var clears = 0

        override fun load(): DurableTunnelCredentials? {
            loads += 1
            return current
        }

        override fun save(credentials: DurableTunnelCredentials) {
            if (failSaves) throw IllegalStateException("storage unavailable")
            saves += 1
            current = credentials
        }

        override fun clearSession() {
            clears += 1
            current = current?.let { DurableTunnelCredentials(it.profile, null) }
        }
    }

    private class FakeSessionApi(
        private val enrollment: EnrollmentResult,
        private val renewalError: Exception? = null,
    ) : TunnelSessionApi {
        var enrollments = 0
        var renewals = 0
        val releasedTokens = mutableListOf<String>()

        override fun enroll(serverUrl: String, accessCode: String, clientName: String): EnrollmentResult {
            assertEquals(SERVER_URL, serverUrl)
            assertEquals(ACCESS_CODE, accessCode)
            assertEquals("Ket Android", clientName)
            enrollments += 1
            return enrollment
        }

        override fun renew(serverUrl: String, token: String): Long {
            assertEquals(SERVER_URL, serverUrl)
            renewals += 1
            renewalError?.let { throw it }
            return 4_000_000_000
        }

        override fun release(serverUrl: String, token: String) {
            assertEquals(SERVER_URL, serverUrl)
            releasedTokens += token
        }
    }

    private fun credentials(manifest: String?): DurableTunnelCredentials =
        DurableTunnelCredentials(TunnelEnrollmentProfile(SERVER_URL, ACCESS_CODE), manifest)

    private fun enrollment(token: String): EnrollmentResult =
        KetControlApi.parseEnrollment(sessionManifest(token))

    private fun sessionManifest(token: String): String = JSONObject()
        .put("session_token", token)
        .put("session_expires_at_epoch_seconds", 4_000_000_000)
        .put(
            "node",
            JSONObject()
                .put("display_name", "Test node")
                .put("location", JSONObject().put("country_name", "Testland")),
        )
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

    companion object {
        private const val SERVER_URL = "https://ket.example.test"
        private val ACCESS_CODE = "A".repeat(32)
    }
}
