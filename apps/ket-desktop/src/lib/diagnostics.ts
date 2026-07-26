import type { ClientIssue, ClientPhase, DesktopState } from "../types";
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
  engineStatus: DesktopState["engine"]["status"];
  currentPhase: ClientPhase;
  nodeId: string | null;
  protocolPreference: ProtocolPreference;
  activeTransportId: string | null;
}

const MAX_DIAGNOSTICS = 80;
const MAX_MESSAGE_LENGTH = 600;
const SECRET_PATTERN = /\b[A-Za-z0-9]{32}\b/g;
const BEARER_PATTERN = /(bearer\s+)[A-Za-z0-9._~+/=-]+/gi;
const TOKEN_PATTERN = /((?:session|access|token|password|auth)(?:[_ -]?(?:token|code))?\s*[:=]\s*)\S+/gi;

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
    "Ket connection diagnostics",
    `generated_at=${generatedAt.toISOString()}`,
    `version=${context.version}`,
    `platform=${context.platform}`,
    `engine_status=${context.engineStatus}`,
    `current_phase=${context.currentPhase}`,
    `node_id=${context.nodeId ?? "none"}`,
    `protocol_preference=${context.protocolPreference}`,
    `active_transport=${context.activeTransportId ?? "none"}`,
    "secrets=redacted_by_design",
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

export function sanitizeDiagnosticMessage(message: string): string {
  return message
    .replace(BEARER_PATTERN, "$1[REDACTED]")
    .replace(TOKEN_PATTERN, "$1[REDACTED]")
    .replace(SECRET_PATTERN, "[REDACTED]")
    .replace(/[\r\n\t]+/g, " ")
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
