import { describe, expect, it } from "vitest";
import {
  appendDiagnostic,
  formatDiagnosticReport,
  sanitizeDiagnosticMessage,
  type ConnectionDiagnostic,
} from "./diagnostics";

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
    const report = formatDiagnosticReport({
      platform: "linux",
      version: "0.2.0",
      engineStatus: "ready",
      currentPhase: "error",
      nodeId: "sg-01",
      protocolPreference: "stealth",
      activeTransportId: "https-stealth-primary",
    }, appendDiagnostic([], failed), new Date("2026-07-26T10:00:03Z"));

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
});
