import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ClientIssue,
  ClientSnapshot,
  DesktopState,
  EnrollmentInput,
} from "../types";
import type { ProtocolId } from "./protocols";

type SnapshotListener = (snapshot: ClientSnapshot) => void;

export interface DesktopBridge {
  state(): Promise<DesktopState>;
  enroll(input: EnrollmentInput): Promise<ClientSnapshot>;
  connect(preferredProtocol: ProtocolId | null): Promise<ClientSnapshot>;
  stop(): Promise<ClientSnapshot>;
  refresh(): Promise<ClientSnapshot>;
  forget(): Promise<ClientSnapshot>;
  subscribe(listener: SnapshotListener): Promise<UnlistenFn>;
}

const tauriBridge: DesktopBridge = {
  state: () => invoke<DesktopState>("desktop_state"),
  enroll: (input) =>
    invoke<ClientSnapshot>("enroll", {
      serverUrl: input.serverUrl,
      accessCode: input.accessCode,
      deviceName: input.deviceName,
    }),
  connect: (preferredProtocol) => invoke<ClientSnapshot>("connect", { preferredProtocol }),
  stop: () => invoke<ClientSnapshot>("stop_tunnel"),
  refresh: () => invoke<ClientSnapshot>("refresh"),
  forget: () => invoke<ClientSnapshot>("forget"),
  subscribe: async (listener) =>
    listen<ClientSnapshot>("ket://snapshot", (event) => listener(event.payload)),
};

const now = () => Math.floor(Date.now() / 1000);

const demoTransports: ClientSnapshot["available_transports"] = [
  { id: "xhttp-primary", display_name: "HTTPS Stealth", protocol: "stealth", network: "tcp" },
  { id: "wg-primary", display_name: "WireGuard TLS", protocol: "wire_guard", network: "tcp" },
  { id: "hy2-primary", display_name: "Hysteria 2", protocol: "hysteria2", network: "udp" },
  { id: "reality-primary", display_name: "VLESS + REALITY", protocol: "vless_xtls_reality", network: "tcp" },
  { id: "ss-primary", display_name: "Shadowsocks 2022", protocol: "shadowsocks2022", network: "tcp_and_udp" },
  { id: "ovpn-primary", display_name: "OpenVPN TLS", protocol: "open_vpn_stunnel", network: "tcp" },
];

function demoSnapshot(
  phase: ClientSnapshot["phase"],
  preferredProtocol: ProtocolId | null = null,
): ClientSnapshot {
  const connected = phase === "connected";
  const activeTransport = demoTransports.find(
    (transport) => transport.protocol === (preferredProtocol ?? "hysteria2"),
  ) ?? demoTransports[0];
  return {
    phase,
    node: phase === "disconnected" ? null : {
      node_id: "fra-01",
      display_name: "Frankfurt 01",
      public_url: "https://de-fra.ket.example",
      location: {
        country_code: "DE",
        country_name: "Germany",
        city: "Frankfurt",
        latitude: 50.1109,
        longitude: 8.6821,
      },
      health: "healthy",
      active_sessions: 27,
      session_capacity: 120,
      capacity_percent: 22.5,
      cpu_load_percent: 18.4,
      memory_used_bytes: 3_865_470_566,
      memory_total_bytes: 25_769_803_776,
      uptime_seconds: 1_247_820,
      observed_at_epoch_seconds: now(),
    },
    available_transports: phase === "disconnected" ? [] : demoTransports,
    preferred_protocol: preferredProtocol,
    active_transport: connected ? activeTransport : null,
    traffic: connected
      ? {
          available: true,
          bytes_sent: 46_281_774,
          bytes_received: 287_341_255,
          online_connections: 1,
          observed_at_epoch_seconds: now(),
        }
      : null,
    handshake_latency_ms: connected ? 42 : null,
    session_expires_at_epoch_seconds: phase === "disconnected" ? null : now() + 3_240,
    connected_at_epoch_seconds: connected ? now() - 1_842 : null,
    reconnect_attempt: 0,
    issue: null,
    updated_at_epoch_seconds: now(),
  };
}

function mockBridge(): DesktopBridge {
  const query = new URLSearchParams(window.location.search);
  let snapshot = demoSnapshot(query.get("demo") === "connected" ? "connected" : "disconnected");
  let configured = snapshot.node !== null;
  const listeners = new Set<SnapshotListener>();
  const publish = (next: ClientSnapshot) => {
    snapshot = next;
    listeners.forEach((listener) => listener(snapshot));
    return snapshot;
  };
  return {
    state: async () => ({
      snapshot,
      configured,
      engine: { binary_available: true, broker_available: true, mode: "privileged_broker" },
      platform: "linux",
      version: "0.1.0",
    }),
    enroll: async ({ serverUrl, accessCode }) => {
      if (!serverUrl.startsWith("https://") || !/^[A-Za-z0-9]{32}$/.test(accessCode)) {
        throw {
          code: "invalid_input",
          message: "Use an HTTPS server URL and a 32-character access code.",
          retryable: false,
        } satisfies ClientIssue;
      }
      configured = true;
      return publish(demoSnapshot("enrolled"));
    },
    connect: async (preferredProtocol) => publish(demoSnapshot("connected", preferredProtocol)),
    stop: async () => publish(demoSnapshot("enrolled", snapshot.preferred_protocol)),
    refresh: async () => {
      if (snapshot.traffic) {
        snapshot = {
          ...snapshot,
          traffic: {
            ...snapshot.traffic,
            bytes_sent: snapshot.traffic.bytes_sent + 81_920,
            bytes_received: snapshot.traffic.bytes_received + 524_288,
            observed_at_epoch_seconds: now(),
          },
          updated_at_epoch_seconds: now(),
        };
      }
      return publish(snapshot);
    },
    forget: async () => {
      configured = false;
      return publish(demoSnapshot("disconnected"));
    },
    subscribe: async (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

export const bridge = window.__TAURI_INTERNALS__ ? tauriBridge : mockBridge();
