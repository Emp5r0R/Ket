import { describe, expect, it } from "vitest";
import { savedProfileCanRestore, startupPresentation, type StartupStage } from "./startup";

describe("desktop startup policy", () => {
  it("accepts only a complete unexpired alphanumeric saved profile", () => {
    const future = 2_000;
    expect(savedProfileCanRestore("https://ket.example", "A".repeat(32), future, 1_000)).toBe(true);
    expect(savedProfileCanRestore("", "A".repeat(32), future, 1_000)).toBe(false);
    expect(savedProfileCanRestore("https://ket.example", "A".repeat(31), future, 1_000)).toBe(false);
    expect(savedProfileCanRestore("https://ket.example", `${"A".repeat(31)}-`, future, 1_000)).toBe(false);
    expect(savedProfileCanRestore("https://ket.example", "A".repeat(32), null, 1_000)).toBe(false);
    expect(savedProfileCanRestore("https://ket.example", "A".repeat(32), 1_000, 1_000)).toBe(false);
    expect(savedProfileCanRestore("https://ket.example", "A".repeat(32), Number.NaN, 1_000)).toBe(false);
  });

  it("never includes profile secrets in startup presentation copy", () => {
    const secret = "S".repeat(32);
    const stages: StartupStage[] = ["runtime", "restoring", "retrying", "blocked", "ready"];
    for (const stage of stages) {
      const copy = startupPresentation(stage);
      expect(`${copy.title}\n${copy.detail}`).not.toContain(secret);
      expect(`${copy.title}\n${copy.detail}`).not.toContain("https://private.example");
      expect(`${copy.title}\n${copy.detail}`).not.toContain("The server echoed private input");
    }
  });

  it("marks only active startup phases as busy", () => {
    expect(startupPresentation("runtime").busy).toBe(true);
    expect(startupPresentation("restoring").busy).toBe(true);
    expect(startupPresentation("retrying").busy).toBe(true);
    expect(startupPresentation("blocked").busy).toBe(false);
    expect(startupPresentation("ready").busy).toBe(false);
  });
});
