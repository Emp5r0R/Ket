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
 * The app's only entry point for reading invites. Implementations must verify the invite before
 * returning anything from it.
 */
fun interface InviteReader {
    /** Verifies [text] at [nowEpochSeconds]; throws [InviteException] on any rejection. */
    fun read(text: String, nowEpochSeconds: Long): InviteTarget
}

/**
 * Reads invites with the shared Rust implementation. [acceptsBaseUrl] decides which locators this
 * client may use (the control client's URL policy); the first acceptable one wins.
 */
class RustInviteReader(private val acceptsBaseUrl: (String) -> Boolean) : InviteReader {
    override fun read(text: String, nowEpochSeconds: Long): InviteTarget {
        val invite = NativeInvite.read(text.trim(), nowEpochSeconds)
        val baseUrl = invite.origins.firstOrNull(acceptsBaseUrl) ?: fail(InviteFailure.UnsupportedLocator)
        return InviteTarget(baseUrl, invite.accessCode, invite.nodeKey, invite.accessExpiresAtEpochSeconds)
    }
}
