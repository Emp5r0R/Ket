package com.ket.android

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.AtomicFile
import java.io.File
import java.io.FileNotFoundException
import java.nio.ByteBuffer
import java.nio.charset.StandardCharsets
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import org.json.JSONObject

internal class TunnelEnrollmentProfile(
    val serverUrl: String,
    val accessCode: String,
    val preferredProtocol: KetProtocol? = null,
) {
    fun matches(other: TunnelEnrollmentProfile): Boolean =
        serverUrl == other.serverUrl && accessCode == other.accessCode

    override fun toString(): String =
        "TunnelEnrollmentProfile(serverUrl=$serverUrl, accessCode=[REDACTED])"
}

internal class DurableTunnelCredentials(
    val profile: TunnelEnrollmentProfile,
    val sessionManifest: String?,
) {
    fun launchSpec(): TunnelLaunchSpec? = sessionManifest?.let { manifest ->
        TunnelLaunchSpec.fromEnrollment(
            profile.serverUrl,
            KetControlApi.parseEnrollment(manifest, requireActiveLease = false),
            profile.preferredProtocol,
        )
    }

    override fun toString(): String =
        "DurableTunnelCredentials(profile=$profile, sessionManifest=${if (sessionManifest == null) "none" else "[REDACTED]"})"
}

internal interface TunnelCredentialStore {
    fun load(): DurableTunnelCredentials?
    fun save(credentials: DurableTunnelCredentials)
    fun clearSession()
}

internal object DurableTunnelCredentialsCodec {
    private const val SCHEMA_VERSION = 1
    private const val MAX_PLAINTEXT_BYTES = 128 * 1024
    private val rootKeys = setOf(
        "schema_version",
        "server_url",
        "access_code",
        "preferred_protocol",
        "session_manifest",
    )

    fun encode(credentials: DurableTunnelCredentials): ByteArray {
        val root = JSONObject()
            .put("schema_version", SCHEMA_VERSION)
            .put("server_url", credentials.profile.serverUrl)
            .put("access_code", credentials.profile.accessCode)
        credentials.profile.preferredProtocol?.let { root.put("preferred_protocol", it.wireName) }
        credentials.sessionManifest?.let { root.put("session_manifest", JSONObject(it)) }
        return root.toString().toByteArray(StandardCharsets.UTF_8).also {
            require(it.size <= MAX_PLAINTEXT_BYTES) { "Saved tunnel credentials are too large" }
        }
    }

    fun decode(encoded: ByteArray): DurableTunnelCredentials {
        require(encoded.isNotEmpty() && encoded.size <= MAX_PLAINTEXT_BYTES) {
            "Saved tunnel credentials have an invalid size"
        }
        val root = JSONObject(String(encoded, StandardCharsets.UTF_8))
        rejectUnknownKeys(root, rootKeys, "saved tunnel credential field")
        require(root.getInt("schema_version") == SCHEMA_VERSION) {
            "Saved tunnel credentials use an unsupported schema"
        }
        val profile = TunnelEnrollmentProfile(
            serverUrl = KetControlApi.normalizeBaseUrl(root.getString("server_url")),
            accessCode = KetControlApi.validateAccessCode(root.getString("access_code")),
            preferredProtocol = root.optString("preferred_protocol")
                .takeIf(String::isNotEmpty)
                ?.let { KetProtocol.fromWireName(it) ?: throw IllegalArgumentException("Saved protocol is unsupported") },
        )
        val manifest = root.optJSONObject("session_manifest")?.toString()
        manifest?.let(KetControlApi::parseEnrollment)
        return DurableTunnelCredentials(profile, manifest)
    }
}

internal object TunnelCredentialEnvelope {
    private const val GCM_TAG_BITS = 128
    private const val GCM_TAG_BYTES = GCM_TAG_BITS / 8
    private const val MAX_ENCRYPTED_BYTES = 160 * 1024
    private const val MIN_ENCRYPTED_BYTES = 4 + 1 + 12 + GCM_TAG_BYTES + 1
    private const val CIPHER_TRANSFORMATION = "AES/GCM/NoPadding"
    private val magic = byteArrayOf('K'.code.toByte(), 'E'.code.toByte(), 'T'.code.toByte(), 1)
    private val authenticatedContext =
        "com.ket.android.always-on-credentials.v1".toByteArray(StandardCharsets.UTF_8)

    fun seal(plaintext: ByteArray, key: SecretKey): ByteArray {
        val cipher = Cipher.getInstance(CIPHER_TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key)
        cipher.updateAAD(authenticatedContext)
        val ciphertext = cipher.doFinal(plaintext)
        require(cipher.iv.size in 12..32) { "The cipher returned an invalid GCM nonce" }
        return ByteBuffer.allocate(magic.size + 1 + cipher.iv.size + ciphertext.size)
            .put(magic)
            .put(cipher.iv.size.toByte())
            .put(cipher.iv)
            .put(ciphertext)
            .array()
    }

    fun open(encrypted: ByteArray, key: SecretKey): ByteArray {
        require(encrypted.size in MIN_ENCRYPTED_BYTES..MAX_ENCRYPTED_BYTES) {
            "Saved tunnel credentials have an invalid encrypted size"
        }
        val buffer = ByteBuffer.wrap(encrypted)
        val encodedMagic = ByteArray(magic.size).also(buffer::get)
        require(encodedMagic.contentEquals(magic)) {
            "Saved tunnel credentials have an invalid header"
        }
        val ivSize = buffer.get().toInt() and 0xff
        require(ivSize in 12..32 && buffer.remaining() > ivSize + GCM_TAG_BYTES) {
            "Saved tunnel credentials have an invalid encrypted envelope"
        }
        val iv = ByteArray(ivSize).also(buffer::get)
        val ciphertext = ByteArray(buffer.remaining()).also(buffer::get)
        val cipher = Cipher.getInstance(CIPHER_TRANSFORMATION)
        cipher.init(
            Cipher.DECRYPT_MODE,
            key,
            javax.crypto.spec.GCMParameterSpec(GCM_TAG_BITS, iv),
        )
        cipher.updateAAD(authenticatedContext)
        return cipher.doFinal(ciphertext)
    }
}

internal class AndroidTunnelCredentialStore private constructor(context: Context) : TunnelCredentialStore {
    private val backingFile = File(context.noBackupFilesDir, FILE_NAME)
    private val atomicFile = AtomicFile(backingFile)

    override fun load(): DurableTunnelCredentials? = synchronized(fileLock) {
        val encrypted = try {
            atomicFile.openRead().use { input ->
                require(input.channel.size() <= MAX_ENCRYPTED_BYTES) {
                    "Saved tunnel credentials are too large"
                }
                input.readBytes()
            }
        } catch (_: FileNotFoundException) {
            return@synchronized null
        }
        DurableTunnelCredentialsCodec.decode(TunnelCredentialEnvelope.open(encrypted, encryptionKey()))
    }

    override fun save(credentials: DurableTunnelCredentials) = synchronized(fileLock) {
        backingFile.parentFile?.mkdirs()
        val encrypted = TunnelCredentialEnvelope.seal(
            DurableTunnelCredentialsCodec.encode(credentials),
            encryptionKey(),
        )
        val output = atomicFile.startWrite()
        try {
            output.write(encrypted)
            atomicFile.finishWrite(output)
        } catch (error: Exception) {
            atomicFile.failWrite(output)
            throw error
        }
    }

    override fun clearSession() = synchronized(fileLock) {
        val current = load() ?: return@synchronized
        if (current.sessionManifest != null) {
            save(DurableTunnelCredentials(current.profile, null))
        }
    }

    private fun encryptionKey(): SecretKey {
        val keyStore = KeyStore.getInstance(KEYSTORE_PROVIDER).apply { load(null) }
        (keyStore.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE_PROVIDER).run {
            init(
                KeyGenParameterSpec.Builder(
                    KEY_ALIAS,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                )
                    .setKeySize(256)
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .build(),
            )
            generateKey()
        }
    }

    companion object {
        private const val FILE_NAME = "always-on-credentials.v1"
        private const val KEYSTORE_PROVIDER = "AndroidKeyStore"
        private const val KEY_ALIAS = "com.ket.android.always-on-credentials.v1"
        private const val MAX_ENCRYPTED_BYTES = 160 * 1024
        private val fileLock = Any()

        @Volatile
        private var instance: AndroidTunnelCredentialStore? = null

        fun get(context: Context): AndroidTunnelCredentialStore =
            instance ?: synchronized(fileLock) {
                instance ?: AndroidTunnelCredentialStore(context.applicationContext).also { instance = it }
            }
    }
}

internal interface TunnelSessionApi {
    fun enroll(serverUrl: String, accessCode: String, clientName: String): EnrollmentResult
    fun renew(serverUrl: String, token: String): Long
    fun release(serverUrl: String, token: String)
}

/** Resolves an app launch, a process restart, or an unflagged system always-on launch. */
internal class DurableTunnelSessionResolver(
    private val api: TunnelSessionApi,
    private val store: TunnelCredentialStore,
) {
    fun resolveForApp(profile: TunnelEnrollmentProfile): TunnelLaunchSpec {
        val saved = store.load()
        if (saved != null && saved.profile.matches(profile)) {
            val updated = DurableTunnelCredentials(profile, saved.sessionManifest)
            if (saved.profile.preferredProtocol != profile.preferredProtocol) store.save(updated)
            return resolveSaved(updated)
        }
        saved?.launchSpec()?.let { previous ->
            runCatching { api.release(previous.controlEndpoint, previous.sessionToken) }
            store.clearSession()
        }
        return enroll(profile)
    }

    fun resolve(pending: TunnelLaunchSpec?): TunnelLaunchSpec {
        pending?.let { return it }
        val saved = store.load()
            ?: throw IllegalStateException("Connect once in Ket before enabling always-on VPN")
        return resolveSaved(saved)
    }

    private fun resolveSaved(initial: DurableTunnelCredentials): TunnelLaunchSpec {
        var saved = initial
        saved.launchSpec()?.let { existing ->
            try {
                api.renew(existing.controlEndpoint, existing.sessionToken)
                return existing
            } catch (error: Exception) {
                if ((error as? KetControlException)?.authorizationLost != true) return existing
                store.clearSession()
                saved = DurableTunnelCredentials(saved.profile, null)
            }
        }

        return enroll(saved.profile)
    }

    private fun enroll(profile: TunnelEnrollmentProfile): TunnelLaunchSpec {
        val result = api.enroll(profile.serverUrl, profile.accessCode, CLIENT_NAME)
        val next = DurableTunnelCredentials(profile, result.manifestJson)
        try {
            store.save(next)
        } catch (error: Exception) {
            runCatching { api.release(profile.serverUrl, result.token) }
            throw error
        }
        return TunnelLaunchSpec.fromEnrollment(profile.serverUrl, result, profile.preferredProtocol)
    }

    companion object {
        const val CLIENT_NAME = KET_ANDROID_CLIENT_NAME
    }
}
