package com.ket.android

import java.time.Instant
import java.util.Locale

internal enum class DiagnosticOutcome {
    Started,
    Succeeded,
    Failed,
    Observed,
}

internal data class AndroidConnectionDiagnostic(
    val recordedAtEpochMilliseconds: Long,
    val elapsedMilliseconds: Long,
    val stage: String,
    val outcome: DiagnosticOutcome,
    val phase: TunnelPhase,
    val protocol: String,
    val transport: String?,
    val code: String?,
    val message: String?,
)

internal object AndroidConnectionDiagnostics {
    private const val MaxEvents = 96
    private const val MaxMessageLength = 600
    private val lock = Any()
    private val events = ArrayDeque<AndroidConnectionDiagnostic>()
    private var startedAtNanos = System.nanoTime()
    private var preferredProtocol = "automatic"

    fun begin(protocol: KetProtocol?, source: String = "app") {
        synchronized(lock) {
            events.clear()
            startedAtNanos = System.nanoTime()
            preferredProtocol = protocol?.wireName ?: "automatic"
            addLocked(
                stage = "connection_$source",
                outcome = DiagnosticOutcome.Started,
                phase = TunnelPhase.Enrolling,
                protocol = preferredProtocol,
                transport = null,
                code = null,
                message = null,
            )
        }
    }

    fun record(
        stage: String,
        outcome: DiagnosticOutcome,
        phase: TunnelPhase,
        protocol: KetProtocol? = null,
        transport: String? = null,
        code: String? = null,
        message: String? = null,
    ) {
        synchronized(lock) {
            addLocked(
                stage = stage,
                outcome = outcome,
                phase = phase,
                protocol = protocol?.wireName ?: preferredProtocol,
                transport = transport,
                code = code,
                message = message,
            )
        }
    }

    fun report(version: String, snapshot: TunnelSnapshot, generatedAt: Instant = Instant.now()): String {
        val retained = synchronized(lock) { events.toList() }
        return buildString {
            appendLine("Ket Android connection diagnostics")
            appendLine("generated_at=$generatedAt")
            appendLine("version=$version")
            appendLine("platform=android")
            appendLine("current_phase=${snapshot.phase.name.lowercase(Locale.ROOT)}")
            appendLine("protocol_preference=${synchronized(lock) { preferredProtocol }}")
            appendLine("active_transport=${sanitizeField(snapshot.transportName).ifBlank { "none" }}")
            appendLine("reconnect_attempt=${snapshot.reconnectAttempt}")
            appendLine("secrets=redacted_by_design")
            appendLine()
            appendLine("events:")
            if (retained.isEmpty()) {
                appendLine("none")
            } else {
                retained.forEachIndexed { index, event ->
                    append("event=${index + 1}")
                    append(" at=${Instant.ofEpochMilli(event.recordedAtEpochMilliseconds)}")
                    append(" elapsed_ms=${event.elapsedMilliseconds}")
                    append(" stage=${event.stage}")
                    append(" outcome=${event.outcome.name.lowercase(Locale.ROOT)}")
                    append(" phase=${event.phase.name.lowercase(Locale.ROOT)}")
                    append(" protocol=${event.protocol}")
                    append(" transport=${event.transport ?: "none"}")
                    event.code?.let { append(" code=$it") }
                    event.message?.let { append(" message=$it") }
                    appendLine()
                }
            }
        }
    }

    internal fun resetForTest() {
        synchronized(lock) {
            events.clear()
            startedAtNanos = System.nanoTime()
            preferredProtocol = "automatic"
        }
    }

    private fun addLocked(
        stage: String,
        outcome: DiagnosticOutcome,
        phase: TunnelPhase,
        protocol: String,
        transport: String?,
        code: String?,
        message: String?,
    ) {
        events += AndroidConnectionDiagnostic(
            recordedAtEpochMilliseconds = System.currentTimeMillis(),
            elapsedMilliseconds = ((System.nanoTime() - startedAtNanos) / 1_000_000).coerceAtLeast(0),
            stage = sanitizeField(stage),
            outcome = outcome,
            phase = phase,
            protocol = sanitizeField(protocol),
            transport = transport?.let(::sanitizeField),
            code = code?.let(::sanitizeField),
            message = message?.let(::sanitizeMessage),
        )
        while (events.size > MaxEvents) events.removeFirst()
    }

    private fun sanitizeField(value: String): String = value
        .replace(Regex("[^A-Za-z0-9_.-]"), "_")
        .take(96)

    private fun sanitizeMessage(value: String): String = value
        .replace(Regex("(?i)(bearer\\s+)[A-Za-z0-9._~+/=-]+")) { "${it.groupValues[1]}[REDACTED]" }
        .replace(Regex("(?i)((?:session|access|token|password|auth)(?:[_ -]?(?:token|code))?\\s*[:=]\\s*)\\S+")) {
            "${it.groupValues[1]}[REDACTED]"
        }
        .replace(Regex("\\b[A-Za-z0-9]{32}\\b"), "[REDACTED]")
        .replace(Regex("[\\r\\n\\t]+"), " ")
        .trim()
        .take(MaxMessageLength)
}
