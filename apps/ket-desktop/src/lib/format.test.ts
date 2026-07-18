import { describe, expect, it } from "vitest";
import { endpointHost, formatBytes, formatDuration, formatLatency } from "./format";

describe("desktop formatting", () => {
  it("formats telemetry without overflowing raw values into the UI", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1_572_864)).toBe("1.50 MB");
    expect(formatDuration(90_000)).toBe("1d 1h");
    expect(formatLatency(42.4)).toBe("42 ms");
  });

  it("reduces a control endpoint to its host for settings", () => {
    expect(endpointHost("https://node.example.com/control")).toBe("node.example.com");
    expect(endpointHost("not a url")).toBe("not a url");
  });
});

