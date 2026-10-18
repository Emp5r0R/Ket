import { formatBytes } from "./format";
import type { TrafficPoint } from "../types";

/** Bytes per second in each direction over the interval ending at `at`. */
export interface RatePoint {
  at: number;
  sent: number;
  received: number;
}

/** How many traffic samples the app keeps for its charts. */
export const TRAFFIC_HISTORY_LENGTH = 40;

/**
 * The speed between each pair of samples. A counter that went down belongs to
 * a new connection, so what it holds counts as moved in that interval.
 */
export function throughput(points: TrafficPoint[]): RatePoint[] {
  const rates: RatePoint[] = [];
  for (let index = 1; index < points.length; index += 1) {
    const before = points[index - 1];
    const after = points[index];
    const seconds = after.at - before.at;
    if (seconds <= 0) continue;
    rates.push({
      at: after.at,
      sent: moved(before.sent, after.sent) / seconds,
      received: moved(before.received, after.received) / seconds,
    });
  }
  return rates;
}

/** The most recent speed, or null before there are two samples. */
export function latestRate(points: TrafficPoint[]): RatePoint | null {
  return throughput(points.slice(-2)).at(-1) ?? null;
}

/** The highest speed in either direction, for a chart's scale. */
export function peakRate(rates: RatePoint[], metric: "sent" | "received"): number {
  return rates.reduce((peak, rate) => Math.max(peak, rate[metric]), 0);
}

export function formatRate(bytesPerSecond: number | null | undefined): string {
  if (bytesPerSecond == null || !Number.isFinite(bytesPerSecond)) return "--";
  return `${formatBytes(bytesPerSecond)}/s`;
}

function moved(before: number, after: number): number {
  return after >= before ? after - before : after;
}
