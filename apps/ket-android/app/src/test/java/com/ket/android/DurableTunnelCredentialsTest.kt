package com.ket.android

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
        val original = credentials(testSessionManifest(OLD_TOKEN), KetProtocol.WireGuardTls)

        val decoded = DurableTunnelCredentialsCodec.decode(
            DurableTunnelCredentialsCodec.encode(original),
        )
        val launch = requireNotNull(decoded.launchSpec())

        assertEquals(SERVER_URL, decoded.profile.serverUrl)
        assertEquals(ACCESS_CODE, decoded.profile.accessCode)
        assertEquals(KetProtocol.WireGuardTls, decoded.profile.preferredProtocol)
        assertEquals(KetProtocol.WireGuardTls, launch.preferredProtocol)
        assertEquals(OLD_TOKEN, launch.sessionToken)
        assertEquals("Hysteria 2", launch.transports.single().displayName)
        assertFalse(decoded.toString().contains(ACCESS_CODE))
        assertFalse(decoded.toString().contains(OLD_TOKEN))
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
        val invalidProtocol = JSONObject(String(DurableTunnelCredentialsCodec.encode(credentials(null))))
            .put("preferred_protocol", "unsupported")
            .toString()
            .toByteArray()

        assertThrows(IllegalArgumentException::class.java) {
            DurableTunnelCredentialsCodec.decode(wrongSchema)
        }
        assertThrows(IllegalArgumentException::class.java) {
            DurableTunnelCredentialsCodec.decode(unknownField)
        }
        assertThrows(IllegalArgumentException::class.java) {
            DurableTunnelCredentialsCodec.decode(invalidProtocol)
        }
        assertThrows(IllegalArgumentException::class.java) {
            KetControlApi.validateAccessCode("A".repeat(31) + "é")
        }
    }

    @Test
    fun `credential envelope hides plaintext and rejects authenticated tampering`() {
        val plaintext = DurableTunnelCredentialsCodec.encode(
            credentials(testSessionManifest(SEALED_TOKEN)),
        )
        val key = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
        val sealed = TunnelCredentialEnvelope.seal(plaintext, key)

        assertFalse(String(sealed).contains(ACCESS_CODE))
        assertFalse(String(sealed).contains(SEALED_TOKEN))
        assertTrue(plaintext.contentEquals(TunnelCredentialEnvelope.open(sealed, key)))

        sealed[sealed.lastIndex] = (sealed.last().toInt() xor 1).toByte()
        assertThrows(AEADBadTagException::class.java) {
            TunnelCredentialEnvelope.open(sealed, key)
        }
    }

    @Test
    fun `explicit app launch takes precedence without reading durable credentials`() {
        val pending = requireNotNull(credentials(testSessionManifest(PENDING_TOKEN)).launchSpec())
        val store = FakeCredentialStore(null)
        val api = FakeSessionApi(enrollment(UNUSED_TOKEN))

        val resolved = DurableTunnelSessionResolver(api, store).resolve(pending)

        assertSame(pending, resolved)
        assertEquals(0, store.loads)
        assertEquals(0, api.renewals)
        assertEquals(0, api.enrollments)
    }

    @Test
    fun `process restart renews and resumes the existing server session`() {
        val store = FakeCredentialStore(credentials(testSessionManifest(EXISTING_TOKEN)))
        val api = FakeSessionApi(enrollment(UNUSED_TOKEN))

        val resolved = DurableTunnelSessionResolver(api, store).resolve(null)

        assertEquals(EXISTING_TOKEN, resolved.sessionToken)
        assertEquals(1, api.renewals)
        assertEquals(0, api.enrollments)
        assertEquals(0, store.saves)
    }

    @Test
    fun `manual reconnect to the saved profile reuses its existing connection slot`() {
        val store = FakeCredentialStore(credentials(testSessionManifest(EXISTING_TOKEN)))
        val api = FakeSessionApi(enrollment(UNUSED_TOKEN))
        val profile = TunnelEnrollmentProfile(SERVER_URL, ACCESS_CODE)

        val resolved = DurableTunnelSessionResolver(api, store).resolveForApp(profile)

        assertEquals(EXISTING_TOKEN, resolved.sessionToken)
        assertEquals(1, api.renewals)
        assertEquals(0, api.enrollments)
        assertEquals(0, api.releasedTokens.size)
    }

    @Test
    fun `manual protocol change persists without consuming another connection slot`() {
        val store = FakeCredentialStore(credentials(testSessionManifest(EXISTING_TOKEN)))
        val api = FakeSessionApi(enrollment(UNUSED_TOKEN))
        val profile = TunnelEnrollmentProfile(SERVER_URL, ACCESS_CODE, KetProtocol.Stealth)

        val resolved = DurableTunnelSessionResolver(api, store).resolveForApp(profile)

        assertEquals(EXISTING_TOKEN, resolved.sessionToken)
        assertEquals(KetProtocol.Stealth, resolved.preferredProtocol)
        assertEquals(KetProtocol.Stealth, store.current?.profile?.preferredProtocol)
        assertEquals(1, api.renewals)
        assertEquals(0, api.enrollments)
        assertEquals(1, store.saves)
    }

    @Test
    fun `blocked control endpoint retains the existing session for transport recovery`() {
        val store = FakeCredentialStore(credentials(testSessionManifest(EXISTING_TOKEN)))
        val api = FakeSessionApi(
            enrollment(UNUSED_TOKEN),
            renewalError = IllegalStateException("control endpoint blocked"),
        )

        val resolved = DurableTunnelSessionResolver(api, store).resolve(null)

        assertEquals(EXISTING_TOKEN, resolved.sessionToken)
        assertEquals(0, api.enrollments)
        assertEquals(0, store.clears)
    }

    @Test
    fun `expired session is replaced from the encrypted enrollment profile`() {
        val store = FakeCredentialStore(credentials(testSessionManifest(EXPIRED_TOKEN)))
        val api = FakeSessionApi(
            enrollment(REPLACEMENT_TOKEN),
            renewalError = KetControlException(401, "expired"),
        )

        val resolved = DurableTunnelSessionResolver(api, store).resolve(null)

        assertEquals(REPLACEMENT_TOKEN, resolved.sessionToken)
        assertEquals(1, api.renewals)
        assertEquals(1, api.enrollments)
        assertEquals(1, store.clears)
        assertEquals(REPLACEMENT_TOKEN, requireNotNull(store.current?.launchSpec()).sessionToken)
    }

    @Test
    fun `always on start creates a session when only enrollment is stored`() {
        val store = FakeCredentialStore(credentials(null))
        val api = FakeSessionApi(enrollment(FRESH_TOKEN))

        val resolved = DurableTunnelSessionResolver(api, store).resolve(null)

        assertEquals(FRESH_TOKEN, resolved.sessionToken)
        assertEquals(1, api.enrollments)
        assertEquals(1, store.saves)
    }

    @Test
    fun `failed durable save releases the newly created server session`() {
        val store = FakeCredentialStore(credentials(null), failSaves = true)
        val api = FakeSessionApi(enrollment(ORPHAN_TOKEN))

        assertThrows(IllegalStateException::class.java) {
            DurableTunnelSessionResolver(api, store).resolve(null)
        }

        assertEquals(listOf(ORPHAN_TOKEN), api.releasedTokens)
    }

    @Test
    fun `unconfigured always on start fails before contacting the server`() {
        val store = FakeCredentialStore(null)
        val api = FakeSessionApi(enrollment(UNUSED_TOKEN))

        val error = assertThrows(IllegalStateException::class.java) {
            DurableTunnelSessionResolver(api, store).resolve(null)
        }

        assertTrue(error.message.orEmpty().contains("Connect once"))
        assertEquals(0, api.enrollments)
        assertNull(store.current)
    }

    @Test
    fun `expired access erases the saved profile before contacting the server`() {
        val expired = DurableTunnelCredentials(
            TunnelEnrollmentProfile(SERVER_URL, ACCESS_CODE),
            testSessionManifest(EXISTING_TOKEN),
            accessExpiresAtEpochSeconds = 1,
        )
        val store = FakeCredentialStore(expired)
        val api = FakeSessionApi(enrollment(UNUSED_TOKEN))

        val error = assertThrows(IllegalStateException::class.java) {
            DurableTunnelSessionResolver(api, store).resolve(null)
        }

        assertTrue(error.message.orEmpty().contains("expired"))
        assertNull(store.current)
        assertEquals(0, api.renewals)
        assertEquals(0, api.enrollments)
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
            current = current?.let {
                DurableTunnelCredentials(it.profile, null, it.accessExpiresAtEpochSeconds)
            }
        }

        override fun clearAll() {
            clears += 1
            current = null
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

    private fun credentials(
        manifest: String?,
        preferredProtocol: KetProtocol? = null,
    ): DurableTunnelCredentials =
        DurableTunnelCredentials(
            TunnelEnrollmentProfile(SERVER_URL, ACCESS_CODE, preferredProtocol),
            manifest,
        )

    private fun enrollment(token: String): EnrollmentResult =
        KetControlApi.parseEnrollment(testSessionManifest(token))

    companion object {
        private const val SERVER_URL = "https://ket.example.test"
        private val ACCESS_CODE = "A".repeat(32)
        private val OLD_TOKEN = testSessionToken('O')
        private val SEALED_TOKEN = testSessionToken('S')
        private val PENDING_TOKEN = testSessionToken('P')
        private val EXISTING_TOKEN = testSessionToken('E')
        private val EXPIRED_TOKEN = testSessionToken('X')
        private val REPLACEMENT_TOKEN = testSessionToken('R')
        private val FRESH_TOKEN = testSessionToken('F')
        private val ORPHAN_TOKEN = testSessionToken('N')
        private val UNUSED_TOKEN = testSessionToken('U')
    }
}
