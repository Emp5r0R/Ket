import { describe, expect, it } from "vitest";
import type { ClientUpdate } from "../types";
import { compareVersions, describeUpdate } from "./updates";

const update = (status: ClientUpdate["status"], extra: Partial<ClientUpdate> = {}): ClientUpdate => ({
  status,
  latest_version: "1.2.0",
  download_url: "https://mirror.example.test/ket",
  ...extra,
});

describe("update advice", () => {
  it("shows nothing for current apps or old servers", () => {
    expect(describeUpdate(null)).toBeNull();
    expect(describeUpdate(update("current"))).toBeNull();
  });

  it("escalates tone and stops allowing dismissal as urgency rises", () => {
    expect(describeUpdate(update("optional"))).toMatchObject({ tone: "info", dismissible: true });
    expect(describeUpdate(update("recommended", { summary: "Fixes" }))).toMatchObject({
      tone: "info",
      dismissible: true,
      detail: "Fixes",
    });
    const security = describeUpdate(update("security", { required_after_epoch_seconds: 1_800_000_000 }));
    expect(security).toMatchObject({ tone: "warning", dismissible: false });
    expect(security?.title).toMatch(/before/);
    expect(describeUpdate(update("security"))?.title).toMatch(/soon/);
    expect(describeUpdate(update("required"))).toMatchObject({
      tone: "danger",
      dismissible: false,
      downloadUrl: "https://mirror.example.test/ket",
    });
  });

  it("compares release versions numerically", () => {
    expect(compareVersions("1.10.0", "1.9.9")).toBeGreaterThan(0);
    expect(compareVersions("1.0.1", "1.0.1")).toBe(0);
    expect(compareVersions("1.0.0", "1.0.1")).toBeLessThan(0);
    expect(compareVersions("dev", "1.0.0")).toBe(0);
  });
});
