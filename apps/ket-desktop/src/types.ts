import type { ProtocolId } from "./lib/protocols";

export type ClientPhase =
  | "disconnected"
  | "enrolling"
  | "enrolled"
  | "probing"
  | "connecting"
  | "connected"
  | "reconnecting"
  | "disconnecting"
  | "error";

export type HealthState = "healthy" | "degraded" | "saturated";

export interface ClientIssue {
  code: string;
  message: string;
  retryable: boolean;
}

export interface NodeLocation {
  country_code: string;
  country_name: string;
  city: string | null;
  latitude: number;
  longitude: number;
}

export interface NodeStatus {
  node_id: string;
  display_name: string;
  public_url: string;
  location: NodeLocation;
  health: HealthState;
  active_sessions: number;
  session_capacity: number;
  capacity_percent: number;
  cpu_load_percent: number | null;
  memory_used_bytes: number | null;
  memory_total_bytes: number | null;
  uptime_seconds: number | null;
  observed_at_epoch_seconds: number;
}

export interface TransportSummary {
  id: string;
  display_name: string;
  protocol: ProtocolId;
  network: string;
}

export interface SessionTraffic {
  available: boolean;
  bytes_sent: number;
  bytes_received: number;
  online_connections: number;
  observed_at_epoch_seconds: number;
}

export type ClientUpdateStatus = "current" | "optional" | "recommended" | "security" | "required";

/** The server's advice for this app (see docs/UPDATES.md). */
export interface ClientUpdate {
  status: ClientUpdateStatus;
  latest_version: string;
  required_after_epoch_seconds?: number | null;
  summary?: string | null;
  download_url: string;
}

/** Where a transport the client left gave up. */
export type FallbackStage = "probe" | "connect" | "connected";

export interface TransportFallback {
  transport: TransportSummary;
  issue: ClientIssue;
  /** Absent from apps before diagnostic reports format 2. */
  stage?: FallbackStage;
  /** How long the attempt ran; null for a tunnel lost after connecting. */
  elapsed_ms?: number | null;
}

/** What a diagnostic report says about this computer (never which network). */
export interface DiagnosticEnvironment {
  network: "wifi" | "ethernet" | "cellular" | "other" | "none" | "unknown";
  system: string;
}

/** Connection settings chosen in Settings, applied to every connection. */
export interface ConnectOptions {
  splitTlsHello: boolean;
  /** Protocols Automatic leaves out; an explicit choice still uses them. */
  automaticExcluded: ProtocolId[];
  /** Addresses and networks reached directly, outside the tunnel. */
  bypass: string[];
}

export interface ClientSnapshot {
  phase: ClientPhase;
  node: NodeStatus | null;
  available_transports: TransportSummary[];
  preferred_protocol: ProtocolId | null;
  active_transport: TransportSummary | null;
  traffic: SessionTraffic | null;
  handshake_latency_ms: number | null;
  session_expires_at_epoch_seconds: number | null;
  access_expires_at_epoch_seconds: number | null;
  connected_at_epoch_seconds: number | null;
  reconnect_attempt: number;
  issue: ClientIssue | null;
  /** The transports the latest connection attempt left, and why. */
  fallbacks: TransportFallback[];
  server_version: string | null;
  client_update: ClientUpdate | null;
  updated_at_epoch_seconds: number;
}

export interface EngineReadiness {
  binary_available: boolean;
  broker_available: boolean;
  mode: "privileged_broker";
  status: "ready" | "permission_required" | "not_installed" | "unavailable";
}

export interface DesktopState {
  snapshot: ClientSnapshot;
  configured: boolean;
  engine: EngineReadiness;
  platform: string;
  version: string;
}

export interface EnrollmentInput {
  serverUrl: string;
  accessCode: string;
  deviceName: string;
  /** Node key pinned from an invite (unpadded Base64URL); null for URL + code entry. */
  nodeKey: string | null;
  /** The invite requires encrypted control requests; never sent in the clear. */
  sealedControl: boolean;
}

/** A locally verified invite, as returned by the native `inspect_invite` command. */
export interface InviteDetails {
  server_url: string;
  access_code: string;
  access_expires_at_epoch_seconds: number | null;
  node_key: string;
  node_fingerprint: string;
  sealed_control: boolean;
}

export interface TrafficPoint {
  at: number;
  sent: number;
  received: number;
}
