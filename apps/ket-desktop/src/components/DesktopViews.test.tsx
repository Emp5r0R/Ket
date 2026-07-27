import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { testDesktopState, testNode, testSnapshot } from "../test/fixtures";
import { MetricsView } from "./MetricsView";
import { NavRail } from "./NavRail";
import { ProtocolsView } from "./ProtocolsView";
import { SettingsView } from "./SettingsView";
import { StatStrip } from "./StatStrip";
import { TrafficChart } from "./TrafficChart";

describe("desktop navigation and protocol guides", () => {
  it("marks exactly one active navigation view and emits selected destinations", () => {
    const onChange = vi.fn();
    render(<NavRail active="metrics" onChange={onChange} />);
    expect(screen.getByRole("navigation", { name: "Primary navigation" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Metrics" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("button", { name: "Connection" })).not.toHaveAttribute("aria-current");
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(onChange).toHaveBeenCalledWith("settings");
  });

  it("renders full protocol guidance, availability, and selection", () => {
    const onSelect = vi.fn();
    render(
      <ProtocolsView
        selected="stealth"
        available={new Set(["stealth", "vless_xtls_reality"])}
        onSelect={onSelect}
      />,
    );
    expect(screen.getByRole("heading", { name: "HTTPS Stealth", level: 1 })).toBeInTheDocument();
    expect(screen.getByText("Offered by node")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Best for" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "How it works" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Connect" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Limits" })).toBeInTheDocument();
    expect(screen.getAllByRole("listitem")).toHaveLength(5);
    fireEvent.click(screen.getByRole("button", { name: /OpenVPN TLS/ }));
    expect(onSelect).toHaveBeenCalledWith("open_vpn_stunnel");
  });

  it("labels an unavailable selected protocol accurately", () => {
    render(
      <ProtocolsView
        selected="wire_guard"
        available={new Set(["stealth"])}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByText("Not offered by node")).toBeInTheDocument();
  });
});

describe("desktop settings", () => {
  it("shows sanitized runtime details and changes theme", () => {
    const onThemeChange = vi.fn();
    render(
      <SettingsView
        state={testDesktopState()}
        serverUrl="https://user:secret@ket.example.test"
        deviceName="Workstation"
        busy={false}
        themePreference="dark"
        onThemeChange={onThemeChange}
        onForget={vi.fn(async () => undefined)}
      />,
    );
    expect(screen.getByText("ket.example.test")).toBeInTheDocument();
    expect(screen.queryByText(/secret/)).not.toBeInTheDocument();
    expect(screen.getByText("Workstation")).toBeInTheDocument();
    expect(screen.getByText("Ket 0.2.1")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Dark" })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(screen.getByRole("radio", { name: "Light" }));
    expect(onThemeChange).toHaveBeenCalledWith("light");
  });

  it("maps every tunnel-service state and protects forget while unavailable", () => {
    const statuses = [
      ["ready", true, "Ready"],
      ["not_installed", false, "Not installed"],
      ["permission_required", false, "Restart required"],
      ["unavailable", false, "Unavailable"],
    ] as const;
    for (const [status, brokerAvailable, label] of statuses) {
      const { unmount } = render(
        <SettingsView
          state={testDesktopState({
            configured: false,
            engine: {
              binary_available: status === "ready",
              broker_available: brokerAvailable,
              mode: "privileged_broker",
              status,
            },
          })}
          serverUrl=""
          deviceName=""
          busy={false}
          themePreference="system"
          onThemeChange={vi.fn()}
          onForget={vi.fn(async () => undefined)}
        />,
      );
      const tunnelServiceRow = screen.getByText("Tunnel service").closest(".settings-row");
      expect(tunnelServiceRow).not.toBeNull();
      expect(within(tunnelServiceRow as HTMLElement).getByText(label)).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Forget server" })).toBeDisabled();
      unmount();
    }
  });

  it("forgets a configured server exactly once", () => {
    const onForget = vi.fn(async () => undefined);
    render(
      <SettingsView
        state={testDesktopState()}
        serverUrl="https://ket.example.test"
        deviceName="Workstation"
        busy={false}
        themePreference="system"
        onThemeChange={vi.fn()}
        onForget={onForget}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Forget server" }));
    expect(onForget).toHaveBeenCalledOnce();
  });
});

describe("desktop telemetry", () => {
  it("renders live node and traffic metrics consistently across views", () => {
    const snapshot = testSnapshot({
      phase: "connected",
      active_transport: {
        id: "stealth-primary",
        display_name: "HTTPS Stealth",
        protocol: "stealth",
        network: "tcp",
      },
      traffic: {
        available: true,
        bytes_sent: 1_024,
        bytes_received: 2_048,
        online_connections: 1,
        observed_at_epoch_seconds: 1_000,
      },
      handshake_latency_ms: 42,
    });
    const { rerender } = render(
      <MetricsView snapshot={snapshot} history={[{ at: 1, sent: 10, received: 20 }]} />,
    );
    expect(screen.getByText("HTTPS Stealth")).toBeInTheDocument();
    expect(screen.getByText("42 ms")).toBeInTheDocument();
    expect(screen.getByText("1.00 KB / 4.00 KB")).toBeInTheDocument();
    expect(screen.getByText("3 session slots available")).toBeInTheDocument();
    expect(screen.getByText("2.00 KB")).toBeInTheDocument();

    rerender(<StatStrip snapshot={snapshot} />);
    const metrics = screen.getByLabelText("Connection metrics");
    expect(within(metrics).getByText("2.00 KB")).toBeInTheDocument();
    expect(within(metrics).getByText("1.00 KB")).toBeInTheDocument();
    expect(within(metrics).getByText("42 ms")).toBeInTheDocument();
    expect(within(metrics).getByText("25%")).toBeInTheDocument();
  });

  it("uses explicit placeholders and clamps invalid visual capacity", () => {
    const snapshot = testSnapshot({
      node: { ...testNode, capacity_percent: 140, cpu_load_percent: null, memory_used_bytes: null, memory_total_bytes: null, uptime_seconds: null },
    });
    const { container } = render(<MetricsView snapshot={snapshot} history={[]} />);
    expect(screen.getAllByText("--").length).toBeGreaterThanOrEqual(5);
    expect(container.querySelector(".capacity-track span")).toHaveStyle({ width: "100%" });
  });

  it("builds no data path for empty history and stable geometry for one or many samples", () => {
    const { container, rerender } = render(<TrafficChart points={[]} metric="received" />);
    expect(screen.getByRole("img", { name: "Received traffic history" })).toBeInTheDocument();
    expect(container.querySelector(".chart-line")).not.toBeInTheDocument();

    rerender(<TrafficChart points={[{ at: 1, sent: 5, received: 10 }]} metric="received" />);
    const single = container.querySelector(".chart-line")?.getAttribute("d");
    expect(single).toBe("M8,75 L512,75");
    expect(container.querySelector(".chart-area")?.getAttribute("d")).toMatch(/ Z$/);

    rerender(
      <TrafficChart
        points={[{ at: 1, sent: 1, received: 30 }, { at: 2, sent: 2, received: 10 }]}
        metric="sent"
      />,
    );
    expect(screen.getByRole("img", { name: "Sent traffic history" })).toBeInTheDocument();
    expect(container.querySelector(".chart-line")?.getAttribute("d")).toBe("M8,142 L512,8");
  });
});
