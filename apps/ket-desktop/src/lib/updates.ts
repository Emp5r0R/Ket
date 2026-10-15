import type { ClientUpdate } from "../types";

export type UpdateTone = "info" | "warning" | "danger";

/** How the app presents the server's update advice. */
export interface UpdateNotice {
  tone: UpdateTone;
  title: string;
  detail: string | null;
  /** Optional and recommended updates can be hidden until the next version. */
  dismissible: boolean;
  downloadUrl: string;
  latestVersion: string;
}

function formatDay(epochSeconds: number): string {
  return new Date(epochSeconds * 1000).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

/** Maps the server's advice to what the app shows; `null` shows nothing. */
export function describeUpdate(update: ClientUpdate | null): UpdateNotice | null {
  if (update === null || update.status === "current") return null;
  const base = {
    detail: update.summary ?? null,
    downloadUrl: update.download_url,
    latestVersion: update.latest_version,
  };
  switch (update.status) {
    case "optional":
      return { ...base, tone: "info", title: `Ket ${update.latest_version} is available`, dismissible: true };
    case "recommended":
      return { ...base, tone: "info", title: `Update recommended: Ket ${update.latest_version}`, dismissible: true };
    case "security":
      return {
        ...base,
        tone: "warning",
        title:
          update.required_after_epoch_seconds == null
            ? `Security update: install Ket ${update.latest_version} soon`
            : `Security update: install Ket ${update.latest_version} before ${formatDay(update.required_after_epoch_seconds)}`,
        dismissible: false,
      };
    case "required":
      return {
        ...base,
        tone: "danger",
        title: `Update required: this server needs Ket ${update.latest_version}`,
        dismissible: false,
      };
  }
}

/** Compares MAJOR.MINOR.PATCH versions; anything else compares as equal. */
export function compareVersions(left: string, right: string): number {
  const parse = (value: string) =>
    /^\d+\.\d+\.\d+$/.test(value) ? value.split(".").map(Number) : null;
  const a = parse(left);
  const b = parse(right);
  if (a === null || b === null) return 0;
  for (let index = 0; index < 3; index += 1) {
    if (a[index] !== b[index]) return a[index] - b[index];
  }
  return 0;
}
