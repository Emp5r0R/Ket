package com.ket.android

import com.ket.android.invite.InviteException
import com.ket.android.invite.InviteFailure
import com.ket.android.invite.InviteReader
import com.ket.android.invite.KotlinInviteReader

/** The app's invite verifier, restricted to locators the control client may reach. */
internal val appInviteReader: InviteReader = KotlinInviteReader { baseUrl ->
    runCatching { KetControlApi.normalizeBaseUrl(baseUrl) }.isSuccess
}

/**
 * What to tell someone whose invite was refused, and what to do about it.
 * Never includes invite or credential text.
 */
internal fun inviteFailureMessage(error: Throwable): String =
    when ((error as? InviteException)?.failure) {
        InviteFailure.Invalid, InviteFailure.TooLarge ->
            "That is not a complete Ket invite. Copy all of it; it starts with ket:invite:"
        InviteFailure.Expired -> "This invite has expired. Ask for a new one."
        InviteFailure.InvalidSignature, InviteFailure.InvalidNodeKey ->
            "This invite was changed or damaged on the way. Ask for it again."
        InviteFailure.UnsupportedVersion, InviteFailure.UnsupportedCapability ->
            "This invite needs a newer version of Ket. Update the app, then try again."
        InviteFailure.WrongApplication -> "This invite is for a different app, not Ket."
        InviteFailure.UnsupportedLocator, InviteFailure.UnsafeLocator ->
            "This invite points to a server address Ket cannot use. Ask for a new invite."
        InviteFailure.NodeIdentityMismatch ->
            "The server could not prove it is the one named in the invite. Do not use it; ask for a new invite."
        null -> "This invite could not be read."
    }
