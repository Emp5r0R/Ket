package com.ket.android.invite

/** What the app needs from a verified invite. The access code is a bearer secret. */
class InviteTarget(
    val serverUrl: String,
    val accessCode: String,
    val nodeKey: NodePublicKey,
    val accessExpiresAtEpochSeconds: Long?,
) {
    override fun toString(): String =
        "InviteTarget(serverUrl=$serverUrl, accessCode=[REDACTED], node=${nodeKey.fingerprint}, " +
            "accessExpiresAt=$accessExpiresAtEpochSeconds)"
}

/**
 * The app's only entry point for reading invites. It isolates the verifier so the planned
 * shared Rust implementation (JNI) can replace [KotlinInviteReader] without UI changes.
 */
fun interface InviteReader {
    /** Verifies [text] at [nowEpochSeconds]; throws [InviteException] on any rejection. */
    fun read(text: String, nowEpochSeconds: Long): InviteTarget
}

/**
 * Kotlin verifier backed by [InviteCapsule]. [acceptsBaseUrl] decides which locators this client
 * may use (the control client's URL policy); the first acceptable one wins.
 */
class KotlinInviteReader(private val acceptsBaseUrl: (String) -> Boolean) : InviteReader {
    override fun read(text: String, nowEpochSeconds: Long): InviteTarget {
        val capsule = InviteCapsule.decodeText(text.trim(), nowEpochSeconds)
        ensure(capsule.supportsControlApi(CONTROL_API_VERSION), InviteFailure.UnsupportedVersion)
        val baseUrl = capsule.locators
            .map { it.baseUrl.trimEnd('/') }
            .firstOrNull(acceptsBaseUrl)
            ?: fail(InviteFailure.UnsupportedLocator)
        return InviteTarget(baseUrl, capsule.accessCode, capsule.nodePublicKey, capsule.accessExpiresAtEpochSeconds)
    }
}
