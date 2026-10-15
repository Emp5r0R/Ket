package com.ket.android.invite

import com.google.crypto.tink.subtle.Ed25519Verify
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.security.GeneralSecurityException
import java.security.MessageDigest
import java.security.SecureRandom
import java.util.Base64

private const val FINGERPRINT_DOMAIN = "KET-NODE-FINGERPRINT-V1"
private const val ENROLLMENT_DOMAIN = "KET-ENROLLMENT-V1"
private const val PUBLIC_KEY_BYTES = 32
private const val SIGNATURE_BYTES = 64
const val ENROLLMENT_NONCE_HEADER = "Ket-Enrollment-Nonce"
const val NODE_SIGNATURE_HEADER = "Ket-Node-Signature"
internal const val ENROLLMENT_NONCE_BYTES = 32

private val base64UrlAlphabet = Regex("^[A-Za-z0-9_-]+$")

/** Exact, canonical unpadded Base64URL; anything else is rejected. */
internal object Base64Url {
    fun encode(value: ByteArray): String = Base64.getUrlEncoder().withoutPadding().encodeToString(value)

    fun decode(value: String): ByteArray {
        ensure(base64UrlAlphabet.matches(value), InviteFailure.Invalid)
        val decoded = try {
            Base64.getUrlDecoder().decode(value)
        } catch (_: IllegalArgumentException) {
            fail(InviteFailure.Invalid)
        }
        ensure(encode(decoded) == value, InviteFailure.Invalid)
        return decoded
    }
}

/** A node's Ed25519 public key as pinned from an invite. */
class NodePublicKey private constructor(private val bytes: ByteArray) {
    val encoded: ByteArray get() = bytes.copyOf()

    /** A short, human-comparable identity such as `1A2B:3C4D:...` (16 bytes). */
    val fingerprint: String by lazy {
        val digest = MessageDigest.getInstance("SHA-256").run {
            update(FINGERPRINT_DOMAIN.toByteArray(Charsets.US_ASCII))
            update(bytes)
            digest()
        }
        digest.take(16).chunked(2).joinToString(":") { pair -> pair.joinToString("") { "%02X".format(it) } }
    }

    /** RFC 8032 Ed25519 verification through Google Tink. */
    internal fun verify(message: ByteArray, signature: ByteArray): Boolean = try {
        signature.size == SIGNATURE_BYTES && run {
            Ed25519Verify(bytes).verify(signature, message)
            true
        }
    } catch (_: GeneralSecurityException) {
        false
    } catch (_: IllegalArgumentException) {
        false
    }

    override fun equals(other: Any?): Boolean = other is NodePublicKey && bytes.contentEquals(other.bytes)

    override fun hashCode(): Int = bytes.contentHashCode()

    override fun toString(): String = "NodePublicKey($fingerprint)"

    companion object {
        fun fromBytes(value: ByteArray): NodePublicKey {
            ensure(value.size == PUBLIC_KEY_BYTES && value.any { it != 0.toByte() }, InviteFailure.InvalidNodeKey)
            return NodePublicKey(value.copyOf())
        }
    }
}

/** A single-use 32-byte enrollment challenge. */
class EnrollmentNonce private constructor(private val bytes: ByteArray) {
    val headerValue: String get() = Base64Url.encode(bytes)

    internal val encoded: ByteArray get() = bytes.copyOf()

    override fun equals(other: Any?): Boolean = other is EnrollmentNonce && bytes.contentEquals(other.bytes)

    override fun hashCode(): Int = bytes.contentHashCode()

    override fun toString(): String = "EnrollmentNonce(..)"

    companion object {
        private val random = SecureRandom()

        fun generate(): EnrollmentNonce = EnrollmentNonce(ByteArray(ENROLLMENT_NONCE_BYTES).also(random::nextBytes))

        fun fromHeaderValue(value: String): EnrollmentNonce {
            val decoded = Base64Url.decode(value)
            ensure(decoded.size == ENROLLMENT_NONCE_BYTES, InviteFailure.Invalid)
            return EnrollmentNonce(decoded)
        }
    }
}

/** Verifies `Ket-Node-Signature` over the exact enrollment response body. */
object EnrollmentProof {
    /**
     * Throws [InviteException] with [InviteFailure.NodeIdentityMismatch] unless [headerValue] is
     * the pinned node's signature over this nonce and exact body.
     */
    fun verify(nodeKey: NodePublicKey, nonce: EnrollmentNonce, body: ByteArray, headerValue: String?) {
        val signature = headerValue?.let { runCatching { Base64Url.decode(it) }.getOrNull() }
        ensure(
            signature != null && signature.size == SIGNATURE_BYTES && nodeKey.verify(signatureInput(nonce, body), signature),
            InviteFailure.NodeIdentityMismatch,
        )
    }

    /** `domain || nonce || u32be(len(body)) || body`. */
    internal fun signatureInput(nonce: EnrollmentNonce, body: ByteArray): ByteArray =
        ByteArrayOutputStream(ENROLLMENT_DOMAIN.length + ENROLLMENT_NONCE_BYTES + 4 + body.size).apply {
            write(ENROLLMENT_DOMAIN.toByteArray(Charsets.US_ASCII))
            write(nonce.encoded)
            write(ByteBuffer.allocate(4).putInt(body.size).array())
            write(body)
        }.toByteArray()
}
