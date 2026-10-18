import { useCallback, useEffect, useState } from "react";
import type { ClientIssue, ShareNetwork, ShareStatus, ShareView } from "../types";
import type { DesktopBridge } from "./bridge";

/** How often the share's device count is refreshed. */
export const SHARE_POLL_MS = 2_000;

type ShareBridge = Pick<DesktopBridge, "listShareNetworks" | "startSharing" | "stopSharing" | "shareStatus">;

export interface Sharing {
  networks: ShareNetwork[];
  view: ShareView | null;
  status: ShareStatus | null;
  /** Why sharing could not start, or why it stopped on its own. */
  notice: string | null;
  busy: boolean;
  refreshNetworks(): Promise<void>;
  start(address: string): Promise<void>;
  stop(): Promise<void>;
}

const NETWORK_KINDS: Record<ShareNetwork["kind"], string> = {
  wifi: "Wi-Fi",
  ethernet: "Ethernet",
  cellular: "Mobile",
  other: "Network",
  none: "Network",
  unknown: "Network",
};

export function networkLabel(network: ShareNetwork): string {
  return `${NETWORK_KINDS[network.kind]} (${network.interface}) · ${network.address}`;
}

export function devicesText(count: number): string {
  if (count === 0) return "No devices connected";
  return count === 1 ? "1 device connected" : `${count} devices connected`;
}

export function shareRefusal(code: string): string {
  switch (code) {
    case "share_network_unavailable":
      return "This computer is no longer on that network. Choose another one.";
    case "share_port_unavailable":
      return "Ket could not open a port on that network. Try again, or choose another network.";
    default:
      return "Sharing could not start. Try again.";
  }
}

export const NETWORK_GONE =
  "Sharing stopped because this computer left the network it was sharing on.";

/**
 * Sharing state for the app: the networks on offer, the running share, and
 * its device count, refreshed every {@link SHARE_POLL_MS} while it runs. The
 * share stops natively when Ket disconnects; the next refresh then clears it
 * here.
 */
export function useSharing(bridge: ShareBridge): Sharing {
  const [networks, setNetworks] = useState<ShareNetwork[]>([]);
  const [view, setView] = useState<ShareView | null>(null);
  const [status, setStatus] = useState<ShareStatus | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refreshNetworks = useCallback(async () => {
    try {
      setNetworks(await bridge.listShareNetworks());
    } catch {
      setNetworks([]);
    }
  }, [bridge]);

  useEffect(() => {
    if (view === null) return;
    let cancelled = false;
    const poll = async () => {
      try {
        const next = await bridge.shareStatus();
        if (cancelled) return;
        if (next === null || next.ended !== null) {
          setView(null);
          setStatus(null);
          if (next?.ended === "network_gone") setNotice(NETWORK_GONE);
          return;
        }
        setStatus(next);
      } catch {
        // The next poll tries again.
      }
    };
    void poll();
    const timer = window.setInterval(() => void poll(), SHARE_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [bridge, view]);

  const start = useCallback(async (address: string) => {
    setBusy(true);
    setNotice(null);
    try {
      const started = await bridge.startSharing(address);
      setView(started);
    } catch (error) {
      setNotice(shareRefusal((error as Partial<ClientIssue>)?.code ?? ""));
      void refreshNetworks();
    } finally {
      setBusy(false);
    }
  }, [bridge, refreshNetworks]);

  const stop = useCallback(async () => {
    setBusy(true);
    try {
      await bridge.stopSharing();
    } finally {
      setView(null);
      setStatus(null);
      setBusy(false);
    }
  }, [bridge]);

  return { networks, view, status, notice, busy, refreshNetworks, start, stop };
}
