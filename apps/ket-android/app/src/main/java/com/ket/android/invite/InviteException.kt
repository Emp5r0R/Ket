package com.ket.android.invite

/** Stable rejection reasons; [code] matches the Rust `InviteError::code` values. */
enum class InviteFailure(val code: String, val message: String) {
    Invalid("invalid_invite", "Invalid invite"),
    TooLarge("invite_too_large", "Invite is too large"),
    UnsupportedVersion("unsupported_invite_version", "Unsupported invite version"),
    WrongApplication("wrong_application", "Invite belongs to another application"),
    UnsupportedCapability("unsupported_capability", "Unsupported invite capability"),
    InvalidSignature("invalid_invite_signature", "Invite signature is invalid"),
    Expired("invite_expired", "Invite has expired"),
    UnsupportedLocator("unsupported_locator", "Unsupported server address in invite"),
    UnsafeLocator("unsafe_locator", "Unsafe server address in invite"),
    InvalidNodeKey("invalid_node_key", "Invalid node key"),
    NodeIdentityMismatch("node_identity_mismatch", "Server could not prove it is the node named in the invite"),
}

/** Every invite and node-identity rejection. Messages never contain invite or credential text. */
class InviteException(val failure: InviteFailure) : IllegalArgumentException(failure.message)

internal fun fail(failure: InviteFailure): Nothing = throw InviteException(failure)

internal fun ensure(condition: Boolean, failure: InviteFailure) {
    if (!condition) fail(failure)
}
