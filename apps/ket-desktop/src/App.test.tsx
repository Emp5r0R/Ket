import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ClientSnapshot } from "./types";

afterEach(() => {
  cleanup();
  localStorage.clear();
  window.history.replaceState({}, "", "/");
  vi.resetModules();
});

describe("Ket desktop shell", () => {
  it("retries only temporary saved-profile failures", async () => {
    const {
      savedEnrollmentIsTerminal,
      savedSessionNeedsReenrollment,
      nextRestoreRetryDelay,
      shouldRetrySavedEnrollment,
    } = await import("./App");

    expect(shouldRetrySavedEnrollment({
      code: "grant_capacity_reached",
      message: "capacity reached",
      retryable: false,
    })).toBe(true);
    expect(shouldRetrySavedEnrollment({
      code: "network_unavailable",
      message: "offline",
      retryable: true,
    })).toBe(true);
    expect(savedEnrollmentIsTerminal({
      code: "grant_expired",
      message: "expired",
      retryable: false,
    })).toBe(true);
    expect(savedEnrollmentIsTerminal({
      code: "unauthorized",
      message: "revoked",
      retryable: false,
    })).toBe(true);
    expect(savedSessionNeedsReenrollment({
      code: "unauthorized",
      message: "expired session",
      retryable: false,
    }, "A".repeat(32), 2_000, 1_000)).toBe(true);
    expect(savedSessionNeedsReenrollment({
      code: "unauthorized",
      message: "expired session",
      retryable: false,
    }, "A".repeat(32), 1_000, 1_000)).toBe(false);
    expect(nextRestoreRetryDelay(0)).toBe(4_000);
    expect(nextRestoreRetryDelay(16_000)).toBe(30_000);
    expect(nextRestoreRetryDelay(30_000)).toBe(30_000);
  });

  it("keeps a retryable saved profile behind the branded restore screen", async () => {
    const expiry = Math.floor(Date.now() / 1_000) + 3_600;
    localStorage.setItem("ket.server-url", "https://saved.example.test");
    localStorage.setItem("ket.access-code", "R".repeat(32));
    localStorage.setItem("ket.access-expires-at", expiry.toString());
    const { bridge } = await import("./lib/bridge");
    const enroll = vi.spyOn(bridge, "enroll").mockRejectedValue({
      code: "network_unavailable",
      message: "request timed out",
      retryable: true,
    });
    const { default: App } = await import("./App");

    render(<App />);

    await waitFor(() => expect(enroll).toHaveBeenCalledTimes(1));
    await waitFor(() => {
      expect(screen.getByRole("status")).toHaveTextContent("Restoring saved server");
      expect(screen.getByRole("status")).toHaveTextContent("Retrying automatically");
      expect(screen.getByRole("status")).not.toHaveTextContent("request timed out");
    });
    expect(screen.queryByLabelText("Server URL")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Access code")).not.toBeInTheDocument();
    expect(screen.queryByText("https://saved.example.test")).not.toBeInTheDocument();
    expect(screen.queryByText("R".repeat(32))).not.toBeInTheDocument();
    expect(localStorage.getItem("ket.server-url")).toBe("https://saved.example.test");
    expect(localStorage.getItem("ket.access-code")).toBe("R".repeat(32));
    expect(localStorage.getItem("ket.access-expires-at")).toBe(expiry.toString());
  });

  it("never mounts stored profile fields while the first restore is pending", async () => {
    const expiry = Math.floor(Date.now() / 1_000) + 3_600;
    const secret = "P".repeat(32);
    localStorage.setItem("ket.server-url", "https://private.example.test");
    localStorage.setItem("ket.access-code", secret);
    localStorage.setItem("ket.access-expires-at", expiry.toString());
    localStorage.setItem("ket.device-name", "Private workstation");
    const { bridge } = await import("./lib/bridge");
    const pendingRestore = new Promise<ClientSnapshot>(() => undefined);
    const enroll = vi.spyOn(bridge, "enroll").mockReturnValue(pendingRestore);
    const { default: App } = await import("./App");
    const { container } = render(<App />);

    await waitFor(() => expect(enroll).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("status")).toHaveTextContent("Restoring saved server");
    expect(screen.queryByLabelText("Server URL")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Access code")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Device name")).not.toBeInTheDocument();
    expect(container).not.toHaveTextContent("https://private.example.test");
    expect(container).not.toHaveTextContent("Private workstation");
    expect(container).not.toHaveTextContent(secret);

  });

  it("clears a terminal saved profile before exposing blank enrollment", async () => {
    const expiry = Math.floor(Date.now() / 1_000) + 3_600;
    localStorage.setItem("ket.server-url", "https://revoked.example.test");
    localStorage.setItem("ket.access-code", "T".repeat(32));
    localStorage.setItem("ket.access-expires-at", expiry.toString());
    localStorage.setItem("ket.node-key", `${"K".repeat(42)}E`);
    const { bridge } = await import("./lib/bridge");
    vi.spyOn(bridge, "enroll").mockRejectedValue({
      code: "unauthorized",
      message: "access was revoked",
      retryable: false,
    });
    const { default: App } = await import("./App");

    render(<App />);

    expect(await screen.findByLabelText("Invite")).toHaveValue("");
    expect(screen.queryByLabelText("Access code")).not.toBeInTheDocument();
    expect(localStorage.getItem("ket.server-url")).toBeNull();
    expect(localStorage.getItem("ket.access-code")).toBeNull();
    expect(localStorage.getItem("ket.access-expires-at")).toBeNull();
    expect(localStorage.getItem("ket.node-key")).toBeNull();
  });

  it("rejects malformed persisted access codes before attempting restore", async () => {
    const expiry = Math.floor(Date.now() / 1_000) + 3_600;
    localStorage.setItem("ket.server-url", "https://saved.example.test");
    localStorage.setItem("ket.access-code", `${"M".repeat(31)}-`);
    localStorage.setItem("ket.access-expires-at", expiry.toString());
    const { bridge } = await import("./lib/bridge");
    const enroll = vi.spyOn(bridge, "enroll");
    const { default: App } = await import("./App");

    render(<App />);

    expect(await screen.findByLabelText("Server URL")).toHaveValue("https://saved.example.test");
    expect(screen.getByLabelText("Access code")).toHaveValue("");
    expect(enroll).not.toHaveBeenCalled();
    expect(localStorage.getItem("ket.access-code")).toBeNull();
    expect(localStorage.getItem("ket.access-expires-at")).toBeNull();
  });

  it("retains a successful profile and access expiry", async () => {
    window.history.replaceState({}, "", "/");
    const { default: App } = await import("./App");
    render(<App />);

    expect(await screen.findByRole("heading", { name: "Choose your node" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Advanced: use a server URL and access code" }));
    fireEvent.change(screen.getByLabelText("Server URL"), {
      target: { value: "https://de-fra.ket.example" },
    });
    const code = screen.getByLabelText("Access code") as HTMLInputElement;
    fireEvent.change(code, { target: { value: "A2345678901234567890123456789012" } });
    fireEvent.click(screen.getByRole("button", { name: "Add server" }));

    expect(await screen.findByRole("heading", { name: "Frankfurt, Germany" })).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByLabelText("Access code")).not.toBeInTheDocument());
    expect(localStorage.getItem("ket.server-url")).toBe("https://de-fra.ket.example");
    expect(localStorage.getItem("ket.access-code")).toBe("A2345678901234567890123456789012");
    expect(Number(localStorage.getItem("ket.access-expires-at"))).toBeGreaterThan(
      Date.now() / 1_000,
    );
    expect(screen.getByText(/access left/)).toBeInTheDocument();
  });

  it("removes an expired saved profile before rendering enrollment", async () => {
    localStorage.setItem("ket.server-url", "https://expired.example.test");
    localStorage.setItem("ket.access-code", "Z".repeat(32));
    localStorage.setItem("ket.access-expires-at", "1");
    localStorage.setItem("ket.node-key", `${"K".repeat(42)}E`);

    const { default: App } = await import("./App");
    render(<App />);

    expect(await screen.findByLabelText("Invite")).toHaveValue("");
    expect(localStorage.getItem("ket.node-key")).toBeNull();
    expect(localStorage.getItem("ket.server-url")).toBeNull();
    expect(localStorage.getItem("ket.access-code")).toBeNull();
    expect(localStorage.getItem("ket.access-expires-at")).toBeNull();
  });

  it("enrolls from a verified invite and pins its node key", async () => {
    const { bridge } = await import("./lib/bridge");
    const enroll = vi.spyOn(bridge, "enroll");
    const { default: App } = await import("./App");
    render(<App />);

    fireEvent.change(await screen.findByLabelText("Invite"), {
      target: { value: `ket:invite:${"Q".repeat(40)}` },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add server" }));

    expect(await screen.findByRole("heading", { name: "Frankfurt, Germany" })).toBeInTheDocument();
    expect(enroll).toHaveBeenCalledWith({
      serverUrl: "https://de-fra.ket.example",
      accessCode: "D".repeat(32),
      deviceName: "Ket desktop",
      nodeKey: `${"A".repeat(42)}E`,
    });
    expect(localStorage.getItem("ket.server-url")).toBe("https://de-fra.ket.example");
    expect(localStorage.getItem("ket.access-code")).toBe("D".repeat(32));
    expect(localStorage.getItem("ket.node-key")).toBe(`${"A".repeat(42)}E`);
  });

  it("rejects an invalid invite without contacting a server", async () => {
    const { bridge } = await import("./lib/bridge");
    const enroll = vi.spyOn(bridge, "enroll");
    const { default: App } = await import("./App");
    render(<App />);

    fireEvent.change(await screen.findByLabelText("Invite"), {
      target: { value: `not-an-invite-${"Q".repeat(40)}` },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add server" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("invalid invite");
    expect(enroll).not.toHaveBeenCalled();
    expect(localStorage.getItem("ket.node-key")).toBeNull();
  });

  it("restores a pinned profile with its node key", async () => {
    const expiry = Math.floor(Date.now() / 1_000) + 3_600;
    const nodeKey = `${"N".repeat(42)}E`;
    localStorage.setItem("ket.server-url", "https://pinned.example.test");
    localStorage.setItem("ket.access-code", "S".repeat(32));
    localStorage.setItem("ket.access-expires-at", expiry.toString());
    localStorage.setItem("ket.node-key", nodeKey);
    const { bridge } = await import("./lib/bridge");
    const enroll = vi.spyOn(bridge, "enroll");
    const { default: App } = await import("./App");
    render(<App />);

    await waitFor(() => expect(enroll).toHaveBeenCalledTimes(1));
    expect(enroll).toHaveBeenCalledWith({
      serverUrl: "https://pinned.example.test",
      accessCode: "S".repeat(32),
      deviceName: "Ket desktop",
      nodeKey,
    });
  });

  it("restores an unexpired profile without asking for the server again", async () => {
    const expiry = Math.floor(Date.now() / 1_000) + 3_600;
    localStorage.setItem("ket.server-url", "https://saved.example.test");
    localStorage.setItem("ket.access-code", "S".repeat(32));
    localStorage.setItem("ket.access-expires-at", expiry.toString());

    const { default: App } = await import("./App");
    render(<App />);

    expect(await screen.findByRole("heading", { name: "Frankfurt, Germany" })).toBeInTheDocument();
    expect(screen.queryByLabelText("Server URL")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Access code")).not.toBeInTheDocument();
    expect(screen.getByText(/access left/)).toBeInTheDocument();
    expect(localStorage.getItem("ket.access-code")).toBe("S".repeat(32));
  });

  it("re-enrolls and resumes the requested tunnel after a session authorization loss", async () => {
    const expiry = Math.floor(Date.now() / 1_000) + 3_600;
    localStorage.setItem("ket.server-url", "https://saved.example.test");
    localStorage.setItem("ket.access-code", "U".repeat(32));
    localStorage.setItem("ket.access-expires-at", expiry.toString());
    localStorage.setItem("ket.protocol-preference", "stealth");
    const connected: ClientSnapshot = {
      phase: "connected",
      node: {
        node_id: "fra-01",
        display_name: "Frankfurt 01",
        public_url: "https://saved.example.test",
        location: {
          country_code: "DE",
          country_name: "Germany",
          city: "Frankfurt",
          latitude: 50.1109,
          longitude: 8.6821,
        },
        health: "healthy",
        active_sessions: 1,
        session_capacity: 10,
        capacity_percent: 10,
        cpu_load_percent: 5,
        memory_used_bytes: 1,
        memory_total_bytes: 2,
        uptime_seconds: 60,
        observed_at_epoch_seconds: expiry - 3_600,
      },
      available_transports: [
        {
          id: "xhttp-primary",
          display_name: "HTTPS Stealth",
          protocol: "stealth",
          network: "tcp",
        },
      ],
      preferred_protocol: "stealth",
      active_transport: {
        id: "xhttp-primary",
        display_name: "HTTPS Stealth",
        protocol: "stealth",
        network: "tcp",
      },
      traffic: null,
      handshake_latency_ms: 20,
      session_expires_at_epoch_seconds: expiry - 3_000,
      access_expires_at_epoch_seconds: expiry,
      connected_at_epoch_seconds: expiry - 3_500,
      reconnect_attempt: 0,
      issue: null,
      updated_at_epoch_seconds: expiry - 3_500,
    };
    const enrolled: ClientSnapshot = {
      ...connected,
      phase: "enrolled",
      active_transport: null,
      connected_at_epoch_seconds: null,
      handshake_latency_ms: null,
    };
    const { bridge } = await import("./lib/bridge");
    let publish: ((snapshot: ClientSnapshot) => void) | undefined;
    vi.spyOn(bridge, "state").mockResolvedValue({
      snapshot: connected,
      configured: true,
      engine: {
        binary_available: true,
        broker_available: true,
        mode: "privileged_broker",
        status: "ready",
      },
      platform: "linux",
      version: "1.0.1",
    });
    vi.spyOn(bridge, "subscribe").mockImplementation(async (listener) => {
      publish = listener;
      return () => undefined;
    });
    const enroll = vi.spyOn(bridge, "enroll").mockResolvedValue(enrolled);
    const connect = vi.spyOn(bridge, "connect").mockResolvedValue(connected);
    const { default: App } = await import("./App");
    render(<App />);
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Liberated"));

    await act(async () => {
      publish?.({
        ...connected,
        phase: "error",
        active_transport: null,
        issue: {
          code: "unauthorized",
          message: "session credentials are invalid",
          retryable: false,
        },
        updated_at_epoch_seconds: connected.updated_at_epoch_seconds + 1,
      });
    });

    await waitFor(() => expect(enroll).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(connect).toHaveBeenCalledWith("stealth"));
    expect(localStorage.getItem("ket.access-code")).toBe("U".repeat(32));
  });

  it("persists an explicit dark theme", async () => {
    const { default: App } = await import("./App");
    const { container } = render(<App />);

    await screen.findByRole("heading", { name: "Choose your node" });
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    fireEvent.click(screen.getByRole("radio", { name: "Dark" }));

    expect(localStorage.getItem("ket.theme")).toBe("dark");
    expect(container.querySelector(".app-shell")).toHaveClass("theme-dark");
  });

  it("shows connected telemetry and navigates to metrics", async () => {
    window.history.replaceState({}, "", "/?demo=connected");
    const { default: App } = await import("./App");
    render(<App />);

    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Liberated"));
    expect(screen.getByRole("img", { name: /Server location: Frankfurt/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Metrics" }));
    expect(screen.getByRole("heading", { name: "Network metrics" })).toBeInTheDocument();
    expect(screen.getByText("23%")).toBeInTheDocument();
    expect(screen.getByText("Hysteria 2")).toBeInTheDocument();
    expect(screen.getByText("42 ms")).toBeInTheDocument();
  });

  it("uses a chosen protocol and opens its learn more page", async () => {
    const { default: App } = await import("./App");
    render(<App />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Advanced: use a server URL and access code" }),
    );
    fireEvent.change(screen.getByLabelText("Server URL"), {
      target: { value: "https://de-fra.ket.example" },
    });
    fireEvent.change(screen.getByLabelText("Access code"), {
      target: { value: "A2345678901234567890123456789012" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add server" }));

    const selector = await screen.findByRole("combobox", { name: "Preferred protocol" });
    fireEvent.click(selector);
    fireEvent.click(screen.getByRole("option", { name: "HTTPS Stealth" }));
    expect(localStorage.getItem("ket.protocol-preference")).toBe("stealth");

    fireEvent.click(screen.getByRole("button", { name: "Learn more" }));
    expect(screen.getByRole("heading", { name: "HTTPS Stealth", level: 1 })).toBeInTheDocument();
    expect(screen.getByText(/XHTTP packet-up/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Connection" }));
    fireEvent.click(screen.getByRole("button", { name: "Connect tunnel" }));
    expect(await screen.findByText("Protected route active")).toBeInTheDocument();
    expect(screen.getAllByText("HTTPS Stealth").length).toBeGreaterThan(0);
  });
});
