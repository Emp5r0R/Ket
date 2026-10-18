/** The addresses and networks kept outside the tunnel, saved on this computer. */

export const MAX_BYPASS_ENTRIES = 64;
const BYPASS_KEY = "ket.bypass";

/** Saved entries; the tunnel service checks them again when connecting. */
export function loadBypass(): string[] {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(BYPASS_KEY) ?? "[]");
    return Array.isArray(saved)
      ? saved.filter((value): value is string => typeof value === "string").slice(0, MAX_BYPASS_ENTRIES)
      : [];
  } catch {
    return [];
  }
}

export function saveBypass(entries: readonly string[]): void {
  try {
    if (entries.length === 0) localStorage.removeItem(BYPASS_KEY);
    else localStorage.setItem(BYPASS_KEY, JSON.stringify(entries));
  } catch {
    // Without storage the list lasts until Ket closes.
  }
}

/** What to tell someone whose entry was refused, by refusal code. */
export function bypassRefusal(code: string): string {
  switch (code) {
    case "bypass_too_wide":
      return "That network is too large. Use a /8 or narrower for IPv4, /16 or narrower for IPv6.";
    case "bypass_special":
      return "Loopback, multicast, and broadcast addresses never use the tunnel.";
    case "bypass_too_many":
      return `Keep at most ${MAX_BYPASS_ENTRIES} entries.`;
    default:
      return "Enter an IP address, such as 203.0.113.5, or a network, such as 192.168.10.0/24.";
  }
}
