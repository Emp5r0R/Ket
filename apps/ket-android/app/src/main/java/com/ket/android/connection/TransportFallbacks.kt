package com.ket.android.connection

import com.ket.android.AndroidTransport
import com.ket.android.KetProtocol

/** A transport a connection attempt tried and left, and why. */
data class TransportFallbackNote(
    val transportName: String,
    val reason: String,
)

/** Keeps the reasons Ket gives for not using the protocols it tried first. */
object TransportFallbacks {
    /** How many reasons a snapshot keeps while a recovery loop runs. */
    const val MAX_NOTES = 8

    fun appended(notes: List<TransportFallbackNote>, note: TransportFallbackNote): List<TransportFallbackNote> =
        (notes + note).takeLast(MAX_NOTES)

    /**
     * The message for a chosen protocol the session no longer offers (the
     * server owner may have limited the invite), or null when it is offered.
     */
    fun preferredUnavailable(transports: List<AndroidTransport>, preferred: KetProtocol?): String? =
        preferred
            ?.takeIf { protocol -> transports.none { it.protocol == protocol } }
            ?.let { "${it.displayName} is not available for this invite; choose Automatic or another protocol" }
}
