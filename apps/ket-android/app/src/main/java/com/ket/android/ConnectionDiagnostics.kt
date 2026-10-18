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
    private const val ReportFormat = 2
    private const val NodeIdShown = 8
    private const val ResolutionStage = "transport_resolution"
    private val TransportStages = setOf("transport_start", "transport_recovery")
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

    fun report(
        version: String,
        snapshot: TunnelSnapshot,
        environment: DiagnosticEnvironment = DiagnosticEnvironment(),
        generatedAt: Instant = Instant.now(),
    ): String {
        val retained = synchronized(lock) { events.toList() }
        return buildString {
            appendLine("Ket diagnostic report")
            appendLine(
                "Share it with the person who runs your server. It contains no addresses, access codes, " +
                    "tokens, or sites you visited.",
            )
            appendLine("format=$ReportFormat")
            appendLine("generated_at=$generatedAt")
            appendLine("app=android $version")
            appendLine("platform=android")
            appendLine("system=${sanitizeField(environment.system)}")
            appendLine("network=${sanitizeField(environment.network)}")
            appendLine("server=${snapshot.node?.let { "#" + sanitizeField(it.id).take(NodeIdShown) } ?: "none"}")
            appendLine("server_version=${sanitizeField(snapshot.serverVersion ?: "unknown")}")
            appendLine("control=${sanitizeField(environment.control)}")
            appendLine("protocol_preference=${synchronized(lock) { preferredProtocol }}")
            appendLine(
                "automatic_skips=" +
                    environment.automaticSkips.map(::sanitizeField).ifEmpty { listOf("none") }.joinToString(","),
            )
            appendLine("bypass_entries=${environment.bypassEntries}")
            appendLine("current_phase=${snapshot.phase.name.lowercase(Locale.ROOT)}")
            appendLine("active_transport=${sanitizeField(snapshot.transportName).ifBlank { "none" }}")
            appendLine("reconnect_attempt=${snapshot.reconnectAttempt}")
            appendLine("secrets=redacted_by_design")
            appendLine()
            appendLine("attempts:")
            val attempts = attempts(retained, snapshot.handshakeLatencyMs)
            if (attempts.isEmpty()) appendLine("none") else attempts.forEach(::appendLine)
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

    /**
     * Each transport the retained events tried, in order: how long it took and how it ended.
     * A start that has not ended yet is left out.
     */
    private fun attempts(events: List<AndroidConnectionDiagnostic>, handshakeLatencyMs: Long?): List<String> {
        val started = mutableMapOf<String, Long>()
        val lines = mutableListOf<String>()
        events.forEach { event ->
            val transport = event.transport ?: return@forEach
            when {
                event.stage in TransportStages && event.outcome == DiagnosticOutcome.Started ->
                    started[transport] = event.elapsedMilliseconds
                event.stage in TransportStages || event.stage == ResolutionStage -> {
                    if (event.outcome != DiagnosticOutcome.Succeeded && event.outcome != DiagnosticOutcome.Failed) {
                        return@forEach
                    }
                    val since = started.remove(transport)
                    lines += buildString {
                        append("attempt=${lines.size + 1} protocol=${event.protocol} transport=$transport")
                        if (event.outcome == DiagnosticOutcome.Succeeded) {
                            append(" outcome=connected")
                            since?.let { append(" elapsed_ms=${event.elapsedMilliseconds - it}") }
                            handshakeLatencyMs?.let { append(" handshake_ms=$it") }
                        } else {
                            append(" stage=${if (event.stage == ResolutionStage) "probe" else "connect"}")
                            append(" outcome=failed")
                            since?.let { append(" elapsed_ms=${event.elapsedMilliseconds - it}") }
                            event.code?.let { append(" code=$it") }
                            event.message?.let { append(" message=$it") }
                        }
                    }
                }
            }
        }
        return lines
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

    /** The same rules, in the same order, as the desktop app; both replay diagnostic-redaction.json. */
    internal fun sanitizeMessage(value: String): String = Redactions
        .fold(value) { text, (pattern, replacement) -> pattern.replace(text, replacement) }
        .replace(Regex("[\\r\\n\\t]+"), " ")
        .replace(Regex(" {2,}"), " ")
        .trim()
        .take(MaxMessageLength)

    private val Redactions: List<Pair<Regex, String>> = listOf(
        Regex("ket:invite:\\S+", RegexOption.IGNORE_CASE) to "[invite]",
        Regex("\\b[a-z][a-z0-9+.-]*://[^\\s\"'<>]+", RegexOption.IGNORE_CASE) to "[url]",
        Regex("(bearer\\s+)[A-Za-z0-9._~+/=-]+", RegexOption.IGNORE_CASE) to "$1[REDACTED]",
        Regex(
            "((?:session|access|token|password|auth)(?:[_ -]?(?:token|code))?\\s*[:=]\\s*)\\S+",
            RegexOption.IGNORE_CASE,
        ) to "$1[REDACTED]",
        Regex("\\b[\\w.+-]+@[\\w-]+(?:\\.[\\w-]+)+\\b") to "[email]",
        Regex(
            "(?<![A-Za-z0-9_+/=-])(?=[A-Za-z0-9_+/=-]*\\d)(?=[A-Za-z0-9_+/=-]*[A-Za-z])" +
                "[A-Za-z0-9_+/=-]{32,}(?![A-Za-z0-9_+/=-])",
        ) to "[REDACTED]",
        Regex("\\[[0-9a-f:.]+(?:%[\\w.-]+)?](?::\\d{1,5})?", RegexOption.IGNORE_CASE) to "[address]",
        Regex("\\b\\d{1,3}(?:\\.\\d{1,3}){3}(?::\\d{1,5})?\\b") to "[address]",
        Regex("(?<![\\w:])(?:[0-9a-f]{0,4}:){2,7}[0-9a-f]{0,4}(?:%[\\w.-]+)?(?![\\w:])", RegexOption.IGNORE_CASE) to
            "[address]",
        Regex(
            "\\b(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\\.)+[a-z][a-z0-9-]{0,61}[a-z0-9](?::\\d{1,5})?\\b",
            RegexOption.IGNORE_CASE,
        ) to "[host]",
    )
}
