package com.ket.android

import com.ket.android.invite.InviteException
import com.ket.android.invite.InviteReader
import com.ket.android.invite.KotlinInviteReader

/** The app's invite verifier, restricted to locators the control client may reach. */
internal val appInviteReader: InviteReader = KotlinInviteReader { baseUrl ->
    runCatching { KetControlApi.normalizeBaseUrl(baseUrl) }.isSuccess
}

/** User-facing copy for an invite rejection; never includes invite or credential text. */
internal fun inviteFailureMessage(error: Throwable): String =
    (error as? InviteException)?.failure?.message ?: "This invite could not be read."
