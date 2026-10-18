import type {
  ClientIssue,
  ClientPhase,
  DesktopState,
  DiagnosticEnvironment,
  TransportFallback,
  TransportSummary,
} from "../types";
import type { ProtocolPreference } from "./protocols";

export type DiagnosticOutcome = "started" | "succeeded" | "failed" | "observed";

export interface ConnectionDiagnostic {
  recordedAtEpochMilliseconds: number;
  operation: string;
  outcome: DiagnosticOutcome;
  phase: ClientPhase;
  protocolPreference: ProtocolPreference;
  activeTransportId: string | null;
  elapsedMilliseconds: number | null;
  issue: ClientIssue | null;
}

export interface DiagnosticContext {
  platform: string;
  version: string;
  /** Null until the app has asked the system. */
  environment: DiagnosticEnvironment | null;
  engineStatus: DesktopState["engine"]["status"];
  currentPhase: ClientPhase;
  nodeId: string | null;
  serverVersion: string | null;
  /**
   * How control requests travel: always sealed to the server's key, sealed
   * when the server offers it (older invites), or in the clear inside TLS.
   */
  control: "sealed" | "sealed_if_offered" | "plain";
  protocolPreference: ProtocolPreference;
  automaticExcluded: readonly string[];
  /** How many entries the bypass list holds; the entries are never reported. */
  bypassEntries: number;
  activeTransport: TransportSummary | null;
  handshakeLatencyMs: number | null;
  reconnectAttempt: number;
  fallbacks: readonly TransportFallback[];
}

const REPORT_FORMAT = 2;
const MAX_DIAGNOSTICS = 80;
const MAX_MESSAGE_LENGTH = 600;
const NODE_ID_SHOWN = 8;

/**
 * Applied in order to every message. The Android app applies the same rules,
 * and both replay apps/vectors/diagnostic-redaction.json.
 */
const REDACTIONS: ReadonlyArray<readonly [RegExp, string]> = [
  [/ket:invite:\S+/gi, "[invite]"],
  [/\b[a-z][a-z0-9+.-]*:\/\/[^\s"'<>]+/gi, "[url]"],
  [/(bearer\s+)[A-Za-z0-9._~+/=-]+/gi, "$1[REDACTED]"],
  [/((?:session|access|token|password|auth)(?:[_ -]?(?:token|code))?\s*[:=]\s*)\S+/gi, "$1[REDACTED]"],
  [/\b[\w.+-]+@[\w-]+(?:\.[\w-]+)+\b/g, "[email]"],
  [/(?<![A-Za-z0-9_+/=-])(?=[A-Za-z0-9_+/=-]*\d)(?=[A-Za-z0-9_+/=-]*[A-Za-z])[A-Za-z0-9_+/=-]{32,}(?![A-Za-z0-9_+/=-])/g, "[REDACTED]"],
  [/\[[0-9a-f:.]+(?:%[\w.-]+)?\](?::\d{1,5})?/gi, "[address]"],
  [/\b\d{1,3}(?:\.\d{1,3}){3}(?::\d{1,5})?\b/g, "[address]"],
  [/(?<![\w:])(?:[0-9a-f]{0,4}:){2,7}[0-9a-f]{0,4}(?:%[\w.-]+)?(?![\w:])/gi, "[address]"],
  [/\b(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z][a-z0-9-]{0,61}[a-z0-9](?::\d{1,5})?\b/gi, "[host]"],
];

export function appendDiagnostic(
  diagnostics: ConnectionDiagnostic[],
  diagnostic: ConnectionDiagnostic,
): ConnectionDiagnostic[] {
  return [...diagnostics, sanitizeDiagnostic(diagnostic)].slice(-MAX_DIAGNOSTICS);
}

export function formatDiagnosticReport(
  context: DiagnosticContext,
  diagnostics: ConnectionDiagnostic[],
  generatedAt = new Date(),
): string {
  const lines = [
    "Ket diagnostic report",
    "Share it with the person who runs your server. It contains no addresses, access codes, tokens, or sites you visited.",
    `format=${REPORT_FORMAT}`,
    `generated_at=${generatedAt.toISOString()}`,
    `app=desktop ${context.version}`,
    `platform=${context.platform}`,
    `system=${field(context.environment?.system ?? "unknown")}`,
    `network=${context.environment?.network ?? "unknown"}`,
    `engine_status=${context.engineStatus}`,
    `server=${context.nodeId === null ? "none" : `#${field(context.nodeId).slice(0, NODE_ID_SHOWN)}`}`,
    `server_version=${field(context.serverVersion ?? "unknown")}`,
    `control=${context.control}`,
    `protocol_preference=${context.protocolPreference}`,
    `automatic_skips=${context.automaticExcluded.length === 0 ? "none" : context.automaticExcluded.map(field).join(",")}`,
    `bypass_entries=${context.bypassEntries}`,
    `current_phase=${context.currentPhase}`,
    `active_transport=${context.activeTransport ? field(context.activeTransport.id) : "none"}`,
    `reconnect_attempt=${context.reconnectAttempt}`,
    "secrets=redacted_by_design",
    "",
    "attempts:",
    ...attemptLines(context),
    "",
    "events:",
  ];

  if (diagnostics.length === 0) {
    lines.push("none");
  } else {
    diagnostics.forEach((diagnostic, index) => {
      const fields = [
        `event=${index + 1}`,
        `at=${new Date(diagnostic.recordedAtEpochMilliseconds).toISOString()}`,
        `operation=${diagnostic.operation}`,
        `outcome=${diagnostic.outcome}`,
        `phase=${diagnostic.phase}`,
        `protocol=${diagnostic.protocolPreference}`,
        `transport=${diagnostic.activeTransportId ?? "none"}`,
      ];
      if (diagnostic.elapsedMilliseconds !== null) {
        fields.push(`elapsed_ms=${diagnostic.elapsedMilliseconds}`);
      }
      if (diagnostic.issue !== null) {
        fields.push(
          `code=${diagnostic.issue.code}`,
          `retryable=${diagnostic.issue.retryable}`,
          `message=${diagnostic.issue.message}`,
        );
      }
      lines.push(fields.join(" "));
    });
  }
  return `${lines.join("\n")}\n`;
}

/** The transports the latest connection left, in order, then the one in use. */
function attemptLines(context: DiagnosticContext): string[] {
  const lines = context.fallbacks.map((fallback, index) => {
    const fields = [
      `attempt=${index + 1}`,
      `protocol=${field(fallback.transport.protocol)}`,
      `transport=${field(fallback.transport.id)}`,
      `stage=${fallback.stage ?? "unknown"}`,
      `outcome=failed`,
    ];
    if (fallback.elapsed_ms !== undefined && fallback.elapsed_ms !== null) {
      fields.push(`elapsed_ms=${Math.max(0, Math.round(fallback.elapsed_ms))}`);
    }
    fields.push(
      `code=${field(fallback.issue.code)}`,
      `retryable=${fallback.issue.retryable}`,
      `message=${sanitizeDiagnosticMessage(fallback.issue.message)}`,
    );
    return fields.join(" ");
  });
  if (context.activeTransport && context.currentPhase === "connected") {
    const fields = [
      `attempt=${lines.length + 1}`,
      `protocol=${field(context.activeTransport.protocol)}`,
      `transport=${field(context.activeTransport.id)}`,
      "outcome=connected",
    ];
    if (context.handshakeLatencyMs !== null) fields.push(`handshake_ms=${context.handshakeLatencyMs}`);
    lines.push(fields.join(" "));
  }
  return lines.length === 0 ? ["none"] : lines;
}

/** One report value: no spaces or line breaks, bounded. */
function field(value: string): string {
  return value.replace(/[^A-Za-z0-9_.:/#()-]/g, "_").slice(0, 96);
}

export function sanitizeDiagnosticMessage(message: string): string {
  return REDACTIONS
    .reduce((text, [pattern, replacement]) => text.replace(pattern, replacement), message)
    .replace(/[\r\n\t]+/g, " ")
    .replace(/ {2,}/g, " ")
    .trim()
    .slice(0, MAX_MESSAGE_LENGTH);
}

function sanitizeDiagnostic(diagnostic: ConnectionDiagnostic): ConnectionDiagnostic {
  return {
    ...diagnostic,
    operation: diagnostic.operation.replace(/[^a-z0-9_-]/gi, "_").slice(0, 48),
    activeTransportId: diagnostic.activeTransportId?.slice(0, 96) ?? null,
    elapsedMilliseconds: diagnostic.elapsedMilliseconds === null
      ? null
      : Math.max(0, Math.round(diagnostic.elapsedMilliseconds)),
    issue: diagnostic.issue === null ? null : {
      code: diagnostic.issue.code.replace(/[^a-z0-9_-]/gi, "_").slice(0, 64),
      message: sanitizeDiagnosticMessage(diagnostic.issue.message),
      retryable: diagnostic.issue.retryable,
    },
  };
}
