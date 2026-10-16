import type { ClientSnapshot, DesktopState, EngineReadiness, NodeStatus } from "../types";

export const testNode: NodeStatus = {
  node_id: "sg-01",
  display_name: "Ket Singapore",
  public_url: "https://ket.example.test",
  location: {
    country_code: "SG",
    country_name: "Singapore",
    city: "Singapore",
    latitude: 1.29,
    longitude: 103.85,
  },
  health: "healthy",
  active_sessions: 1,
  session_capacity: 4,
  capacity_percent: 25,
  cpu_load_percent: 3.2,
  memory_used_bytes: 1_024,
  memory_total_bytes: 4_096,
  uptime_seconds: 3_661,
  observed_at_epoch_seconds: 1_000,
};

export const readyEngine: EngineReadiness = {
  binary_available: true,
  broker_available: true,
  mode: "privileged_broker",
  status: "ready",
};

export function testSnapshot(overrides: Partial<ClientSnapshot> = {}): ClientSnapshot {
  return {
    phase: "enrolled",
    node: testNode,
    available_transports: [
      {
        id: "stealth-primary",
        display_name: "HTTPS Stealth",
        protocol: "stealth",
        network: "tcp",
      },
      {
        id: "reality-primary",
        display_name: "VLESS + REALITY",
        protocol: "vless_xtls_reality",
        network: "tcp",
      },
    ],
    preferred_protocol: null,
    active_transport: null,
    traffic: null,
    handshake_latency_ms: null,
    session_expires_at_epoch_seconds: 2_000,
    access_expires_at_epoch_seconds: 3_600,
    connected_at_epoch_seconds: null,
    reconnect_attempt: 0,
    fallbacks: [],
    issue: null,
    server_version: null,
    client_update: null,
    updated_at_epoch_seconds: 1_000,
    ...overrides,
  };
}

export function testDesktopState(overrides: Partial<DesktopState> = {}): DesktopState {
  return {
    snapshot: testSnapshot(),
    configured: true,
    engine: readyEngine,
    platform: "linux",
    version: "1.0.1",
    ...overrides,
  };
}
