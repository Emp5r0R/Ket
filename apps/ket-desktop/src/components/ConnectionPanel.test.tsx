import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import type { ClientSnapshot, EngineReadiness } from "../types";
import { ConnectionPanel } from "./ConnectionPanel";

const reconnecting: ClientSnapshot = {
  phase: "reconnecting",
  node: {
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
    cpu_load_percent: 3,
    memory_used_bytes: 1,
    memory_total_bytes: 4,
    uptime_seconds: 60,
    observed_at_epoch_seconds: 1,
  },
  available_transports: [
    {
      id: "reality-primary",
      display_name: "VLESS + REALITY",
      protocol: "vless_xtls_reality",
      network: "tcp",
    },
  ],
  preferred_protocol: "vless_xtls_reality",
  active_transport: {
    id: "reality-primary",
    display_name: "VLESS + REALITY",
    protocol: "vless_xtls_reality",
    network: "tcp",
  },
  traffic: null,
  handshake_latency_ms: null,
  session_expires_at_epoch_seconds: 120,
  connected_at_epoch_seconds: null,
  reconnect_attempt: 1,
  issue: null,
  updated_at_epoch_seconds: 1,
};

const ready: EngineReadiness = {
  binary_available: true,
  broker_available: true,
  mode: "privileged_broker",
  status: "ready",
};

it("offers an enabled cancel action while reconnecting", () => {
  const onStop = vi.fn(async () => undefined);
  render(
    <ConnectionPanel
      snapshot={reconnecting}
      engine={ready}
      busy={false}
      preference="vless_xtls_reality"
      onPreferenceChange={vi.fn()}
      onLearnMore={vi.fn()}
      onConnect={vi.fn(async () => undefined)}
      onStop={onStop}
    />,
  );

  const cancel = screen.getByRole("button", { name: "Cancel connection attempt" });
  expect(cancel).toBeEnabled();
  fireEvent.click(cancel);
  expect(onStop).toHaveBeenCalledOnce();
  expect(screen.getByLabelText("Preferred protocol")).toBeDisabled();
  expect(screen.getByText("Testing available routes")).toBeInTheDocument();
});
