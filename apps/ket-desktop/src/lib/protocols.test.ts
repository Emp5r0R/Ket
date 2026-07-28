import { describe, expect, it } from "vitest";
import { autoProtocol, isProtocolId, protocolCatalog, protocolInfo } from "./protocols";

describe("protocol catalog contract", () => {
  it("contains every implemented client protocol exactly once", () => {
    expect(protocolCatalog.map(({ id }) => id)).toEqual([
      "stealth",
      "wire_guard",
      "hysteria2",
      "xor_scrambled",
      "vless_xtls_reality",
      "shadowsocks2022",
      "open_vpn_stunnel",
    ]);
    expect(new Set(protocolCatalog.map(({ id }) => id)).size).toBe(protocolCatalog.length);
  });

  it("provides complete actionable copy for every protocol", () => {
    expect(autoProtocol.label).toBe("Automatic");
    expect(autoProtocol.shortInstruction).not.toHaveLength(0);
    for (const protocol of protocolCatalog) {
      expect(protocolInfo(protocol.id)).toBe(protocol);
      expect(protocol.label.trim()).not.toHaveLength(0);
      expect(protocol.shortInstruction).toMatch(/\.$/);
      expect(protocol.bestFor).toMatch(/\.$/);
      expect(protocol.operation).toMatch(/\.$/);
      expect(protocol.steps.length).toBeGreaterThanOrEqual(3);
      expect(protocol.limitations.length).toBeGreaterThanOrEqual(2);
      expect(protocol.steps.every((step) => step.trim().endsWith("."))).toBe(true);
      expect(protocol.limitations.every((limit) => limit.trim().endsWith("."))).toBe(true);
    }
  });

  it("rejects unknown, auto, malformed, and case-shifted identifiers", () => {
    for (const protocol of protocolCatalog) expect(isProtocolId(protocol.id)).toBe(true);
    for (const value of ["", "auto", "ikev2", "stealth ", "STEALTH", "__proto__"]) {
      expect(isProtocolId(value)).toBe(false);
    }
    expect(() => protocolInfo("unknown" as never)).toThrow("Unsupported protocol: unknown");
  });
});
