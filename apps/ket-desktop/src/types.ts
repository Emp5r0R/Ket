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
  protocol: string;
  network: string;
}

export interface SessionTraffic {
  available: boolean;
  bytes_sent: number;
  bytes_received: number;
  online_connections: number;
  observed_at_epoch_seconds: number;
}

export interface ClientSnapshot {
  phase: ClientPhase;
  node: NodeStatus | null;
  active_transport: TransportSummary | null;
  traffic: SessionTraffic | null;
  handshake_latency_ms: number | null;
  session_expires_at_epoch_seconds: number | null;
  connected_at_epoch_seconds: number | null;
  reconnect_attempt: number;
  issue: ClientIssue | null;
  updated_at_epoch_seconds: number;
}

export interface EngineReadiness {
  binary_available: boolean;
  broker_available: boolean;
  mode: "privileged_broker";
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
}

export interface TrafficPoint {
  at: number;
  sent: number;
  received: number;
}
