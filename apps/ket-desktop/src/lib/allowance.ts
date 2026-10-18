import type { AllowanceStatus } from "../types";
import { formatBytes } from "./format";

const EACH = { day: "a day", week: "a week", month: "a month" } as const;
const THIS = { day: "today", week: "this week", month: "this month" } as const;

/** Periods follow the UTC calendar on the server, so reset times are shown in UTC. */
const RESET_FORMAT = new Intl.DateTimeFormat("en-GB", {
  day: "numeric",
  month: "short",
  year: "numeric",
  hour: "2-digit",
  minute: "2-digit",
  timeZone: "UTC",
});

export interface AllowanceView {
  used: string;
  limit: string;
  /** How much is used, 0-100, for the bar. */
  percent: number;
  usedUp: boolean;
  /** "Used this month", for the heading. */
  period: string;
  /** "50 GB a month, shared by every device using this invite". */
  terms: string;
  resets: string;
}

export function allowanceView(status: AllowanceStatus): AllowanceView {
  const usedUp = status.used_bytes >= status.bytes;
  const percent = status.bytes > 0 ? (status.used_bytes / status.bytes) * 100 : 100;
  return {
    used: formatBytes(status.used_bytes),
    limit: formatBytes(status.bytes),
    percent: Math.min(Math.max(percent, 0), 100),
    usedUp,
    period: `Used ${THIS[status.period]}`,
    terms: `${formatBytes(status.bytes)} ${EACH[status.period]}, shared by every device using this invite`,
    resets: `${RESET_FORMAT.format(new Date(status.resets_at_epoch_seconds * 1_000))} UTC`,
  };
}
