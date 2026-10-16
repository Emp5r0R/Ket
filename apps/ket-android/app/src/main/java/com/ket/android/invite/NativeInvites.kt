package com.ket.android.invite

/** The control API version this app speaks; invites must offer it. */
const val CONTROL_API_VERSION = 1
const val INVITE_TEXT_PREFIX = "ket:invite:"

/**
 * The Rust invite implementation (`crates/ket-invite-android`), the same code the server and
 * desktop run. Only this file knows the JNI layout; everything else uses [InviteReader],
 * [NodePublicKey], and [EnrollmentProof].
 */
internal object NativeInvites {
    init {
        System.loadLibrary("ket_invite_jni")
        check(controlApiVersion() == CONTROL_API_VERSION.toLong()) {
            "The invite library speaks control API ${controlApiVersion()}, the app $CONTROL_API_VERSION"
        }
    }

    /** Failure: `[code]`. Success: `[null, nodeKey, fingerprint, accessCode, expiresAt?, origin...]`. */
    @JvmStatic
    external fun readInvite(text: String, nowEpochSeconds: Long): Array<String?>?

    /** The fingerprint of a valid node key, or null. */
    @JvmStatic
    external fun nodeKeyFingerprint(nodeKey: String): String?

    /** Null when [nodeKey] signed this [nonce] and exact [body], otherwise the rejection code. */
    @JvmStatic
    external fun verifyEnrollment(nodeKey: String, nonce: String, body: ByteArray, signature: String?): String?

    @JvmStatic
    external fun controlApiVersion(): Long
}

/** A verified invite as the native reader returns it. */
internal class NativeInvite private constructor(
    val nodeKey: NodePublicKey,
    val accessCode: String,
    val accessExpiresAtEpochSeconds: Long?,
    val origins: List<String>,
) {
    companion object {
        private const val FIRST_ORIGIN = 5

        /** Throws [InviteException] with the reference error code on any rejection. */
        fun read(text: String, nowEpochSeconds: Long): NativeInvite {
            val row = NativeInvites.readInvite(text, nowEpochSeconds) ?: fail(InviteFailure.Invalid)
            row.firstOrNull()?.let { fail(InviteFailure.fromCode(it)) }
            ensure(row.size > FIRST_ORIGIN, InviteFailure.Invalid)
            val nodeKey = row[1] ?: fail(InviteFailure.Invalid)
            val fingerprint = row[2] ?: fail(InviteFailure.Invalid)
            return NativeInvite(
                nodeKey = NodePublicKey.verified(nodeKey, fingerprint),
                accessCode = row[3] ?: fail(InviteFailure.Invalid),
                accessExpiresAtEpochSeconds = row[4]?.toLong(),
                origins = row.drop(FIRST_ORIGIN).filterNotNull(),
            )
        }
    }
}
