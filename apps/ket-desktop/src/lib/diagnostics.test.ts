import { describe, expect, it } from "vitest";
import redaction from "../../../vectors/diagnostic-redaction.json";
import {
  appendDiagnostic,
  formatDiagnosticReport,
  sanitizeDiagnosticMessage,
  type ConnectionDiagnostic,
  type DiagnosticContext,
} from "./diagnostics";

const context: DiagnosticContext = {
  platform: "linux",
  version: "0.2.0",
  environment: { network: "wifi", system: "Ubuntu 24.04 x86_64" },
  engineStatus: "ready",
  currentPhase: "error",
  nodeId: "sg-01-abcdefgh-ijkl",
  serverVersion: "1.2.0",
  control: "sealed",
  protocolPreference: "stealth",
  automaticExcluded: [],
  bypassEntries: 2,
  activeTransport: null,
  handshakeLatencyMs: null,
  reconnectAttempt: 0,
  fallbacks: [],
};

const failed: ConnectionDiagnostic = {
  recordedAtEpochMilliseconds: Date.parse("2026-07-26T10:00:00Z"),
  operation: "connect",
  outcome: "failed",
  phase: "error",
  protocolPreference: "stealth",
  activeTransportId: "https-stealth-primary",
  elapsedMilliseconds: 2_451.4,
  issue: {
    code: "transport_failed",
    message: "Bearer top-secret token=ABCDEFGHIJKLMNOPQRSTUVWXYZ123456",
    retryable: true,
  },
};

describe("connection diagnostics", () => {
  it("formats technical context without retaining credentials", () => {
    const report = formatDiagnosticReport(
      context,
      appendDiagnostic([], failed),
      new Date("2026-07-26T10:00:03Z"),
    );

    expect(report).toContain("operation=connect outcome=failed");
    expect(report).toContain("code=transport_failed retryable=true");
    expect(report).toContain("elapsed_ms=2451");
    expect(report).not.toContain("top-secret");
    expect(report).not.toContain("ABCDEFGHIJKLMNOPQRSTUVWXYZ123456");
    expect(report).toContain("[REDACTED]");
  });

  it("bounds the retained event history", () => {
    let diagnostics: ConnectionDiagnostic[] = [];
    for (let index = 0; index < 100; index += 1) {
      diagnostics = appendDiagnostic(diagnostics, {
        ...failed,
        recordedAtEpochMilliseconds: index,
      });
    }
    expect(diagnostics).toHaveLength(80);
    expect(diagnostics[0].recordedAtEpochMilliseconds).toBe(20);
  });

  it("flattens and bounds arbitrary failure text", () => {
    const message = `line one\npassword: secret-value\t${"x".repeat(900)}`;
    const sanitized = sanitizeDiagnosticMessage(message);
    expect(sanitized).not.toContain("secret-value");
    expect(sanitized).not.toContain("\n");
    expect(sanitized.length).toBeLessThanOrEqual(600);
  });

  it("names the app, system, network kind, and server without identifying them", () => {
    const report = formatDiagnosticReport(context, []);
    expect(report).toContain("format=2");
    expect(report).toContain("app=desktop 0.2.0");
    expect(report).toContain("system=Ubuntu_24.04_x86_64");
    expect(report).toContain("network=wifi");
    expect(report).toContain("server=#sg-01-ab\n");
    expect(report).toContain("server_version=1.2.0");
    expect(report).toContain("control=sealed");
    expect(report).toContain("automatic_skips=none");
    expect(report).toContain("bypass_entries=2");
    expect(report).toContain("attempts:\nnone");
    expect(formatDiagnosticReport({ ...context, environment: null }, [])).toContain("network=unknown");
  });

  it("lists each transport the connection tried, why it left, and the one in use", () => {
    const report = formatDiagnosticReport({
      ...context,
      currentPhase: "connected",
      automaticExcluded: ["open_vpn_stunnel", "wire_guard"],
      activeTransport: { id: "https-stealth-primary", display_name: "HTTPS Stealth", protocol: "stealth", network: "tcp" },
      handshakeLatencyMs: 812,
      fallbacks: [{
        transport: { id: "reality-primary", display_name: "VLESS + REALITY", protocol: "vless_xtls_reality", network: "tcp" },
        issue: { code: "transport_failed", message: "dial 203.0.113.5:443 timed out", retryable: true },
        stage: "probe",
        elapsed_ms: 12_034.6,
      }],
    }, []);
    expect(report).toContain("automatic_skips=open_vpn_stunnel,wire_guard");
    expect(report).toContain(
      "attempt=1 protocol=vless_xtls_reality transport=reality-primary stage=probe outcome=failed elapsed_ms=12035 code=transport_failed retryable=true message=dial [address] timed out",
    );
    expect(report).toContain(
      "attempt=2 protocol=stealth transport=https-stealth-primary outcome=connected handshake_ms=812",
    );
    expect(report).not.toContain("203.0.113.5");
  });

  it.each(redaction.cases)("redacts messages like the Android app: $name", ({ input, output }) => {
    expect(sanitizeDiagnosticMessage(input)).toBe(output);
  });
});
