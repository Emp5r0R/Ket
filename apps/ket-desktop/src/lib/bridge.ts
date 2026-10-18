import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ClientIssue,
  ClientSnapshot,
  ConnectOptions,
  DesktopState,
  DiagnosticEnvironment,
  EnrollmentInput,
  InviteDetails,
  ShareNetwork,
  ShareStatus,
  ShareView,
} from "../types";
import type { ProtocolId } from "./protocols";

type SnapshotListener = (snapshot: ClientSnapshot) => void;

export interface DesktopBridge {
  state(): Promise<DesktopState>;
  /** The operating system and network kind, for a diagnostic report. */
  diagnosticEnvironment(): Promise<DiagnosticEnvironment>;
  /** Checks a bypass entry; resolves to the form it is kept in. */
  checkBypassEntry(entry: string): Promise<string>;
  inspectInvite(invite: string): Promise<InviteDetails>;
  /** Decodes and verifies an invite QR image natively; bytes travel as a raw IPC body. */
  decodeInviteQr(image: Uint8Array): Promise<InviteDetails>;
  enroll(input: EnrollmentInput): Promise<ClientSnapshot>;
  connect(preferredProtocol: ProtocolId | null, options: ConnectOptions): Promise<ClientSnapshot>;
  stop(): Promise<ClientSnapshot>;
  refresh(): Promise<ClientSnapshot>;
  forget(): Promise<ClientSnapshot>;
  subscribe(listener: SnapshotListener): Promise<UnlistenFn>;
  /** The networks this computer can share its connection on now. */
  listShareNetworks(): Promise<ShareNetwork[]>;
  /** Starts sharing on one of this computer's addresses, with a new password. */
  startSharing(address: string): Promise<ShareView>;
  stopSharing(): Promise<void>;
  /** Null when sharing is off. */
  shareStatus(): Promise<ShareStatus | null>;
}

const tauriBridge: DesktopBridge = {
  state: () => invoke<DesktopState>("desktop_state"),
  diagnosticEnvironment: () => invoke<DiagnosticEnvironment>("diagnostic_context"),
  checkBypassEntry: (entry) => invoke<string>("check_bypass_entry", { entry }),
  inspectInvite: (invite) => invoke<InviteDetails>("inspect_invite", { invite }),
  decodeInviteQr: (image) => invoke<InviteDetails>("decode_invite_qr", image),
  enroll: (input) =>
    invoke<ClientSnapshot>("enroll", {
      serverUrl: input.serverUrl,
      accessCode: input.accessCode,
      deviceName: input.deviceName,
      nodeKey: input.nodeKey,
      sealedControl: input.sealedControl,
    }),
  connect: (preferredProtocol, options) =>
    invoke<ClientSnapshot>("connect", {
      preferredProtocol,
      splitTlsHello: options.splitTlsHello,
      automaticExcluded: options.automaticExcluded,
      bypass: options.bypass,
    }),
  stop: () => invoke<ClientSnapshot>("stop_tunnel"),
  refresh: () => invoke<ClientSnapshot>("refresh"),
  forget: () => invoke<ClientSnapshot>("forget"),
  subscribe: async (listener) =>
    listen<ClientSnapshot>("ket://snapshot", (event) => listener(event.payload)),
  listShareNetworks: () => invoke<ShareNetwork[]>("list_share_networks"),
  startSharing: (address) => invoke<ShareView>("start_sharing", { address }),
  stopSharing: () => invoke<void>("stop_sharing"),
  shareStatus: () => invoke<ShareStatus | null>("share_status"),
};

const DEMO_QR = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect width="8" height="8" fill="#fff"/><path d="M1 1h2v2H1zM5 1h2v2H5zM1 5h2v2H1zM4 4h1v1H4z" fill="#000"/></svg>';

const now = () => Math.floor(Date.now() / 1000);
const nextMonthUtc = () => {
  const today = new Date();
  return Date.UTC(today.getUTCFullYear(), today.getUTCMonth() + 1, 1) / 1000;
};

const demoTransports: ClientSnapshot["available_transports"] = [
  { id: "xhttp-primary", display_name: "HTTPS Stealth", protocol: "stealth", network: "tcp" },
  { id: "wg-primary", display_name: "WireGuard TLS", protocol: "wire_guard", network: "tcp" },
  { id: "hy2-primary", display_name: "Hysteria 2", protocol: "hysteria2", network: "udp" },
  { id: "hy2-xor-primary", display_name: "Hysteria XOR", protocol: "xor_scrambled", network: "udp" },
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
    allowance: connected
      ? {
          bytes: 53_687_091_200,
          period: "month",
          used_bytes: 13_314_398_618,
          resets_at_epoch_seconds: nextMonthUtc(),
        }
      : null,
    handshake_latency_ms: connected ? 42 : null,
    session_expires_at_epoch_seconds: phase === "disconnected" ? null : now() + 3_240,
    access_expires_at_epoch_seconds: phase === "disconnected" ? null : now() + 86_400,
    connected_at_epoch_seconds: connected ? now() - 1_842 : null,
    reconnect_attempt: 0,
    fallbacks: [],
    issue: null,
    server_version: null,
    client_update: null,
    updated_at_epoch_seconds: now(),
  };
}

function mockBridge(): DesktopBridge {
  const query = new URLSearchParams(window.location.search);
  const demoPhase = query.get("demo");
  const initialPhase: ClientSnapshot["phase"] = demoPhase === "connected"
    ? "connected"
    : demoPhase === "reconnecting"
      ? "reconnecting"
      : demoPhase === "error"
        ? "error"
        : demoPhase === "enrolled"
          ? "enrolled"
          : "disconnected";
  let snapshot = demoSnapshot(
    initialPhase,
  );
  if (demoPhase === "error") {
    snapshot = {
      ...snapshot,
      issue: {
        code: "transport_failed",
        message: "The selected transport could not establish a protected route.",
        retryable: true,
      },
    };
  }
  let configured = snapshot.node !== null;
  let sharing: ShareView | null = null;
  const listeners = new Set<SnapshotListener>();
  const publish = (next: ClientSnapshot) => {
    snapshot = next;
    listeners.forEach((listener) => listener(snapshot));
    return snapshot;
  };
  // A demo page (?demo=...) moves some traffic so the speed charts can be
  // seen working; tests never set it.
  if (demoPhase !== null) {
    window.setInterval(() => {
      const traffic = snapshot.traffic;
      if (snapshot.phase !== "connected" || traffic === null) return;
      const burst = Math.random() < 0.3 ? 6 : 1;
      publish({
        ...snapshot,
        traffic: {
          ...traffic,
          bytes_received: traffic.bytes_received + Math.round(burst * (300_000 + Math.random() * 900_000)),
          bytes_sent: traffic.bytes_sent + Math.round(burst * (20_000 + Math.random() * 90_000)),
          observed_at_epoch_seconds: traffic.observed_at_epoch_seconds + 3,
        },
      });
    }, 1_000);
  }
  return {
    state: async () => ({
      snapshot,
      configured,
      engine: {
        binary_available: true,
        broker_available: true,
        mode: "privileged_broker",
        status: "ready",
      },
      platform: "linux",
      version: "1.2.0",
    }),
    diagnosticEnvironment: async () => ({ network: "wifi", system: "Demo" }),
    checkBypassEntry: async (entry) => {
      const trimmed = entry.trim();
      // The demo accepts plain-looking entries; the app checks them in Rust.
      if (!/^[0-9a-fA-F.:]+(\/\d{1,3})?$/.test(trimmed)) {
        throw { code: "bypass_invalid", message: "not an IP address or network", retryable: false } satisfies ClientIssue;
      }
      return trimmed;
    },
    inspectInvite: async (invite) => {
      if (!invite.startsWith("ket:invite:")) {
        throw {
          code: "invalid_invite",
          message: "invalid invite",
          retryable: false,
        } satisfies ClientIssue;
      }
      return {
        server_url: "https://de-fra.ket.example",
        access_code: "D".repeat(32),
        access_expires_at_epoch_seconds: now() + 86_400,
        node_key: "A".repeat(42) + "E",
        node_fingerprint: "DEMO:0000:0000:0000:0000:0000:0000:0000",
        sealed_control: true,
      };
    },
    decodeInviteQr: async (image) => {
      if (image.length === 0) {
        throw {
          code: "qr_not_found",
          message: "no QR code was found in that image",
          retryable: false,
        } satisfies ClientIssue;
      }
      return {
        server_url: "https://de-fra.ket.example",
        access_code: "D".repeat(32),
        access_expires_at_epoch_seconds: now() + 86_400,
        node_key: "A".repeat(42) + "E",
        node_fingerprint: "DEMO:0000:0000:0000:0000:0000:0000:0000",
        sealed_control: true,
      };
    },
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
    stop: async () => {
      sharing = null;
      return publish(demoSnapshot("enrolled", snapshot.preferred_protocol));
    },
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
      sharing = null;
      configured = false;
      return publish(demoSnapshot("disconnected"));
    },
    subscribe: async (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    listShareNetworks: async () => [
      { interface: "wlan0", address: "192.168.1.24", kind: "wifi" },
      { interface: "eth0", address: "10.0.0.8", kind: "ethernet" },
    ],
    startSharing: async (address) => {
      sharing = {
        details: {
          network: { interface: "wlan0", address, kind: "wifi" },
          port: 39740,
          username: "ket",
          password: "demo7pass2xy",
        },
        proxy_url: `http://ket:demo7pass2xy@${address}:39740`,
        qr_svg: DEMO_QR,
      };
      return sharing;
    },
    stopSharing: async () => {
      sharing = null;
    },
    shareStatus: async () =>
      sharing === null ? null : { devices: 1, refused_sign_ins: 0, tunnel: snapshot.phase === "connected", ended: null },
  };
}

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

export const bridge = window.__TAURI_INTERNALS__ ? tauriBridge : mockBridge();
