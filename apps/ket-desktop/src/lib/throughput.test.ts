import { describe, expect, it } from "vitest";
import { formatRate, latestRate, peakRate, throughput } from "./throughput";

describe("throughput", () => {
  it("turns running totals into speeds per second", () => {
    const rates = throughput([
      { at: 100, sent: 0, received: 0 },
      { at: 110, sent: 1_000, received: 20_000 },
      { at: 115, sent: 1_500, received: 30_000 },
    ]);
    expect(rates).toEqual([
      { at: 110, sent: 100, received: 2_000 },
      { at: 115, sent: 100, received: 2_000 },
    ]);
    expect(peakRate(rates, "received")).toBe(2_000);
  });

  it("counts a counter that started again as moved in that interval", () => {
    expect(throughput([
      { at: 100, sent: 9_000, received: 9_000 },
      { at: 110, sent: 500, received: 1_000 },
    ])).toEqual([{ at: 110, sent: 50, received: 100 }]);
  });

  it("skips samples that did not move forward in time", () => {
    expect(throughput([
      { at: 100, sent: 0, received: 0 },
      { at: 100, sent: 10, received: 10 },
    ])).toEqual([]);
  });

  it("has no latest speed before there are two samples", () => {
    expect(latestRate([])).toBeNull();
    expect(latestRate([{ at: 1, sent: 1, received: 1 }])).toBeNull();
    expect(latestRate([
      { at: 1, sent: 0, received: 0 },
      { at: 2, sent: 0, received: 0 },
      { at: 4, sent: 2_048, received: 4_096 },
    ])).toEqual({ at: 4, sent: 1_024, received: 2_048 });
  });

  it("formats speeds", () => {
    expect(formatRate(null)).toBe("--");
    expect(formatRate(0)).toBe("0 B/s");
    expect(formatRate(1_572_864)).toBe("1.50 MB/s");
  });
});
