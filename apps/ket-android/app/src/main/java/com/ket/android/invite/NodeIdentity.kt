package com.ket.android.invite

import java.security.SecureRandom
import java.util.Base64

const val ENROLLMENT_NONCE_HEADER = "Ket-Enrollment-Nonce"
const val NODE_SIGNATURE_HEADER = "Ket-Node-Signature"
internal const val ENROLLMENT_NONCE_BYTES = 32

private val base64UrlAlphabet = Regex("^[A-Za-z0-9_-]+$")

/** A node's Ed25519 public key as pinned from an invite, validated by the Rust implementation. */
class NodePublicKey private constructor(
    /** Unpadded Base64URL, the form persisted with a saved profile. */
    val base64Url: String,
    /** A short, human-comparable identity such as `1A2B:3C4D:...`. */
    val fingerprint: String,
    /**
     * The invite offered sealed control, so session requests to this node are never sent in the
     * clear. Without it, requests are sealed when the server offers that and signed otherwise.
     */
    val sealedControl: Boolean = false,
) {
    override fun equals(other: Any?): Boolean =
        other is NodePublicKey && base64Url == other.base64Url && sealedControl == other.sealedControl

    override fun hashCode(): Int = 31 * base64Url.hashCode() + sealedControl.hashCode()

    override fun toString(): String = "NodePublicKey($fingerprint)"

    /** This key with sealed control required (or not), for restoring a saved profile. */
    fun withSealedControl(required: Boolean): NodePublicKey =
        if (required == sealedControl) this else NodePublicKey(base64Url, fingerprint, required)

    companion object {
        /** Throws [InviteException] with [InviteFailure.InvalidNodeKey] unless [value] is a valid key. */
        fun fromBase64Url(value: String): NodePublicKey =
            NodePublicKey(value, NativeInvites.nodeKeyFingerprint(value) ?: fail(InviteFailure.InvalidNodeKey))

        fun fromBytes(value: ByteArray): NodePublicKey = fromBase64Url(encodeBase64Url(value))

        /** A key the native reader already validated. */
        internal fun verified(base64Url: String, fingerprint: String, sealedControl: Boolean) =
            NodePublicKey(base64Url, fingerprint, sealedControl)
    }
}

/** A single-use 32-byte enrollment challenge. */
class EnrollmentNonce private constructor(val headerValue: String) {
    override fun equals(other: Any?): Boolean = other is EnrollmentNonce && headerValue == other.headerValue

    override fun hashCode(): Int = headerValue.hashCode()

    override fun toString(): String = "EnrollmentNonce(..)"

    companion object {
        private val random = SecureRandom()

        fun generate(): EnrollmentNonce =
            EnrollmentNonce(encodeBase64Url(ByteArray(ENROLLMENT_NONCE_BYTES).also(random::nextBytes)))

        /** Exactly 32 bytes of canonical unpadded Base64URL. */
        fun fromHeaderValue(value: String): EnrollmentNonce {
            ensure(base64UrlAlphabet.matches(value), InviteFailure.Invalid)
            val decoded = try {
                Base64.getUrlDecoder().decode(value)
            } catch (_: IllegalArgumentException) {
                fail(InviteFailure.Invalid)
            }
            ensure(decoded.size == ENROLLMENT_NONCE_BYTES && encodeBase64Url(decoded) == value, InviteFailure.Invalid)
            return EnrollmentNonce(value)
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
        NativeInvites.verifyEnrollment(nodeKey.base64Url, nonce.headerValue, body, headerValue)
            ?.let { fail(InviteFailure.fromCode(it)) }
    }
}

private fun encodeBase64Url(value: ByteArray): String = Base64.getUrlEncoder().withoutPadding().encodeToString(value)
