import { fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { describe, expect, it, vi } from "vitest";
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
  allowance: null,
  handshake_latency_ms: null,
  session_expires_at_epoch_seconds: 120,
  access_expires_at_epoch_seconds: 3_600,
  connected_at_epoch_seconds: null,
  reconnect_attempt: 1,
  issue: null,
  fallbacks: [],
  server_version: null,
  client_update: null,
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
      onOpenDiagnostics={vi.fn()}
    />,
  );

  const cancel = screen.getByRole("button", { name: "Cancel connection attempt" });
  expect(cancel).toBeEnabled();
  fireEvent.click(cancel);
  expect(onStop).toHaveBeenCalledOnce();
  expect(screen.getByRole("combobox", { name: "Preferred protocol" })).toBeDisabled();
  expect(screen.getByText("Testing available routes")).toBeInTheDocument();
});

describe("connection controls", () => {
  const renderPanel = (
    snapshot: ClientSnapshot,
    engine: EngineReadiness = ready,
    overrides: Partial<ComponentProps<typeof ConnectionPanel>> = {},
  ) => {
    const callbacks = {
      onPreferenceChange: vi.fn(),
      onLearnMore: vi.fn(),
      onConnect: vi.fn(async () => undefined),
      onStop: vi.fn(async () => undefined),
      onOpenDiagnostics: vi.fn(),
    };
    const result = render(
      <ConnectionPanel
        snapshot={snapshot}
        engine={engine}
        busy={false}
        preference="auto"
        {...callbacks}
        {...overrides}
      />,
    );
    return { ...result, ...callbacks };
  };

  it("renders no controls without an enrolled node", () => {
    const { container } = renderPanel({ ...reconnecting, node: null });
    expect(container).toBeEmptyDOMElement();
  });

  it("connects from an enrolled state and opens the first offered protocol guide", () => {
    const { onConnect, onLearnMore } = renderPanel({
      ...reconnecting,
      phase: "enrolled",
      active_transport: null,
      preferred_protocol: null,
      reconnect_attempt: 0,
    });
    expect(screen.getByText("Connect to get liberated")).toBeInTheDocument();
    expect(screen.getByText("You are under surveillance")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Learn more" }));
    expect(onLearnMore).toHaveBeenCalledWith("vless_xtls_reality");
    fireEvent.click(screen.getByRole("button", { name: "Connect tunnel" }));
    expect(onConnect).toHaveBeenCalledOnce();
  });

  it("disconnects a connected route and locks protocol selection", () => {
    const { onStop } = renderPanel(
      { ...reconnecting, phase: "connected" },
      ready,
      { preference: "vless_xtls_reality" },
    );
    expect(screen.getByText("Liberated")).toBeInTheDocument();
    expect(screen.getByText("Protected route active")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Preferred protocol" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Disconnect tunnel" }));
    expect(onStop).toHaveBeenCalledOnce();
  });

  it("maps tunnel service readiness to actionable warnings and disables connect", () => {
    const cases: [EngineReadiness["status"], string][] = [
      ["permission_required", "Restart Ket from the application menu to activate tunnel access."],
      ["not_installed", "Tunnel service is not installed."],
      ["unavailable", "Tunnel service is installed but is not responding."],
    ];
    for (const [status, message] of cases) {
      const { unmount } = renderPanel(
        { ...reconnecting, phase: "enrolled" },
        { binary_available: false, broker_available: false, mode: "privileged_broker", status },
      );
      expect(screen.getByRole("status")).toHaveTextContent(message);
      expect(screen.getByRole("button", { name: "Connect tunnel" })).toBeDisabled();
      unmount();
    }
  });

  it("distinguishes missing engines and opens diagnostics for a connection failure", () => {
    const { onOpenDiagnostics } = renderPanel(
      {
        ...reconnecting,
        phase: "error",
        issue: { code: "transport_failed", message: "Route validation failed", retryable: true },
      },
      { ...ready, binary_available: false },
    );
    expect(screen.getByRole("status")).toHaveTextContent("Transport engines are not installed.");
    expect(screen.getByRole("alert")).toHaveTextContent("Route validation failed");
    expect(screen.getByText("Connection failed")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Troubleshoot" }));
    expect(onOpenDiagnostics).toHaveBeenCalledOnce();
  });

  it("uses stable copy for every lifecycle phase", () => {
    const labels: [ClientSnapshot["phase"], string][] = [
      ["disconnected", "You are being watched"],
      ["enrolling", "Adding server"],
      ["enrolled", "Connect to get liberated"],
      ["probing", "Testing route"],
      ["connecting", "Connecting"],
      ["connected", "Liberated"],
      ["reconnecting", "Reconnecting"],
      ["disconnecting", "Disconnecting"],
      ["error", "Connection failed"],
    ];
    const { rerender } = renderPanel({ ...reconnecting, phase: "disconnected" });
    for (const [phase, label] of labels) {
      rerender(
        <ConnectionPanel
          snapshot={{ ...reconnecting, phase }}
          engine={ready}
          busy={false}
          preference="auto"
          onPreferenceChange={vi.fn()}
          onLearnMore={vi.fn()}
          onConnect={vi.fn(async () => undefined)}
          onStop={vi.fn(async () => undefined)}
          onOpenDiagnostics={vi.fn()}
        />,
      );
      expect(screen.getByText(label)).toBeInTheDocument();
    }
  });
});
