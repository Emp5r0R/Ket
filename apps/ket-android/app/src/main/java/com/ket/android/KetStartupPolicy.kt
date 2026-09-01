package com.ket.android

internal enum class KetStartupMode {
    Ready,
    RestoringProfile,
}

internal data class KetStartupDecision(
    val mode: KetStartupMode,
    val requestSavedProfileConnection: Boolean,
    val observeExistingConnection: Boolean,
)

internal data class KetStartupPresentation(val title: String, val detail: String)

/** Keeps durable credentials out of the UI while an existing profile is restored. */
internal object KetStartupPolicy {
    fun presentation(): KetStartupPresentation = KetStartupPresentation(
        title = "Restoring saved server",
        detail = "Starting Ket securely",
    )

    fun initial(hasSavedProfile: Boolean, phase: TunnelPhase): KetStartupDecision {
        if (!hasSavedProfile || phase == TunnelPhase.Connected || phase == TunnelPhase.Stopping) {
            return KetStartupDecision(
                mode = KetStartupMode.Ready,
                requestSavedProfileConnection = false,
                observeExistingConnection = false,
            )
        }
        val existingAttempt = phase in setOf(
            TunnelPhase.Enrolling,
            TunnelPhase.Connecting,
            TunnelPhase.Reconnecting,
        )
        return KetStartupDecision(
            mode = KetStartupMode.RestoringProfile,
            requestSavedProfileConnection = !existingAttempt,
            observeExistingConnection = existingAttempt,
        )
    }

    fun restoreFinished(restoreStarted: Boolean, phase: TunnelPhase): Boolean =
        restoreStarted && phase in setOf(TunnelPhase.Connected, TunnelPhase.Failed)
}
