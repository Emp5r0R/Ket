import { act, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SharePanel } from "./SharePanel";
import { ShareBanner } from "./ShareBanner";
import {
  NETWORK_GONE,
  SHARE_POLL_MS,
  devicesText,
  networkLabel,
  shareRefusal,
  useSharing,
  type Sharing,
} from "../lib/sharing";
import type { ShareStatus, ShareView } from "../types";

const VIEW: ShareView = {
  details: {
    network: { interface: "wlan0", address: "192.168.1.24", kind: "wifi" },
    port: 39740,
    username: "ket",
    password: "abcdefghjkmn",
  },
  proxy_url: "http://ket:abcdefghjkmn@192.168.1.24:39740",
  qr_svg: "<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
};

function status(overrides: Partial<ShareStatus> = {}): ShareStatus {
  return { devices: 2, refused_sign_ins: 0, tunnel: true, ended: null, ...overrides };
}

function sharing(overrides: Partial<Sharing> = {}): Sharing {
  return {
    networks: [
      { interface: "wlan0", address: "192.168.1.24", kind: "wifi" },
      { interface: "eth0", address: "10.0.0.8", kind: "ethernet" },
    ],
    view: null,
    status: null,
    notice: null,
    busy: false,
    refreshNetworks: vi.fn(async () => undefined),
    start: vi.fn(async () => undefined),
    stop: vi.fn(async () => undefined),
    ...overrides,
  };
}

function fakeBridge(statuses: (ShareStatus | null)[]) {
  return {
    listShareNetworks: vi.fn(async () => [VIEW.details.network]),
    startSharing: vi.fn(async () => VIEW),
    stopSharing: vi.fn(async () => undefined),
    shareStatus: vi.fn(async () => (statuses.length > 1 ? statuses.shift()! : statuses[0])),
  };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("sharing wording", () => {
  it("names networks, devices, and refusals plainly", () => {
    expect(networkLabel(VIEW.details.network)).toBe("Wi-Fi (wlan0) · 192.168.1.24");
    expect(devicesText(0)).toBe("No devices connected");
    expect(devicesText(1)).toBe("1 device connected");
    expect(devicesText(3)).toBe("3 devices connected");
    expect(shareRefusal("share_network_unavailable")).toMatch(/no longer on that network/);
    expect(shareRefusal("share_port_unavailable")).toMatch(/could not open a port/);
    expect(shareRefusal("anything")).toMatch(/could not start/);
  });
});

describe("share panel", () => {
  it("is off until started, warns what sharing means, and needs a connection", () => {
    const props = sharing();
    const { rerender } = render(<SharePanel sharing={props} connected={false} />);
    expect(screen.getByText(/not encrypted by Ket/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /start sharing/i })).toBeDisabled();
    expect(screen.getByText(/connect ket first/i)).toBeInTheDocument();
    expect(props.refreshNetworks).toHaveBeenCalled();

    rerender(<SharePanel sharing={props} connected />);
    fireEvent.change(screen.getByLabelText("Network"), { target: { value: "10.0.0.8" } });
    fireEvent.click(screen.getByRole("button", { name: /start sharing/i }));
    expect(props.start).toHaveBeenCalledWith("10.0.0.8");
  });

  it("says when there is no network to share on", () => {
    render(<SharePanel sharing={sharing({ networks: [] })} connected />);
    expect(screen.getByText(/not on a local network it can share on/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /start sharing/i })).toBeNull();
  });

  it("shows how to connect, the device count, and refused guesses while sharing", () => {
    const props = sharing({ view: VIEW, status: status({ refused_sign_ins: 3 }) });
    render(<SharePanel sharing={props} connected />);
    expect(screen.getByRole("img", { name: /qr code/i })).toHaveAttribute(
      "src",
      expect.stringContaining("data:image/svg+xml"),
    );
    expect(screen.getByText("192.168.1.24")).toBeInTheDocument();
    expect(screen.getByText("39740")).toBeInTheDocument();
    expect(screen.getByText("abcdefghjkmn")).toBeInTheDocument();
    expect(screen.getByText("2 devices connected")).toBeInTheDocument();
    expect(screen.getByText(/3 sign-ins with a wrong password were refused/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /stop sharing/i }));
    expect(props.stop).toHaveBeenCalled();
  });

  it("says when devices wait for Ket to reconnect", () => {
    render(<SharePanel sharing={sharing({ view: VIEW, status: status({ tunnel: false }) })} connected />);
    expect(screen.getByText(/paused while ket reconnects/i)).toBeInTheDocument();
  });
});

describe("share banner", () => {
  it("appears only while sharing and opens the details", () => {
    const onOpenDetails = vi.fn();
    const { rerender } = render(<ShareBanner sharing={sharing()} onOpenDetails={onOpenDetails} />);
    expect(screen.queryByText(/sharing on/i)).toBeNull();

    const props = sharing({ view: VIEW, status: status({ devices: 1 }) });
    rerender(<ShareBanner sharing={props} onOpenDetails={onOpenDetails} />);
    expect(screen.getByText("Sharing on 192.168.1.24:39740")).toBeInTheDocument();
    expect(screen.getByText(/1 device connected/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Details" }));
    expect(onOpenDetails).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: /stop/i }));
    expect(props.stop).toHaveBeenCalled();
  });
});

describe("useSharing", () => {
  it("starts, follows the device count, and clears when the share ends on its own", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const bridge = fakeBridge([status({ devices: 1 }), status({ ended: "network_gone" })]);
    const { result } = renderHook(() => useSharing(bridge));

    await act(async () => result.current.start("192.168.1.24"));
    expect(bridge.startSharing).toHaveBeenCalledWith("192.168.1.24");
    expect(result.current.view).toEqual(VIEW);
    await waitFor(() => expect(result.current.status?.devices).toBe(1));

    await act(async () => {
      vi.advanceTimersByTime(SHARE_POLL_MS);
    });
    await waitFor(() => expect(result.current.view).toBeNull());
    expect(result.current.notice).toBe(NETWORK_GONE);
  });

  it("clears when Ket disconnected and the share stopped with it", async () => {
    const bridge = fakeBridge([null]);
    const { result } = renderHook(() => useSharing(bridge));
    await act(async () => result.current.start("192.168.1.24"));
    await waitFor(() => expect(result.current.view).toBeNull());
    expect(result.current.notice).toBeNull();
  });

  it("explains a share that could not start", async () => {
    const bridge = fakeBridge([null]);
    bridge.startSharing.mockRejectedValueOnce({ code: "share_port_unavailable", message: "", retryable: true });
    const { result } = renderHook(() => useSharing(bridge));
    await act(async () => result.current.start("192.168.1.24"));
    expect(result.current.view).toBeNull();
    expect(result.current.notice).toMatch(/could not open a port/);
    expect(bridge.listShareNetworks).toHaveBeenCalled();
  });

  it("stops on request", async () => {
    const bridge = fakeBridge([status()]);
    const { result } = renderHook(() => useSharing(bridge));
    await act(async () => result.current.start("192.168.1.24"));
    await act(async () => result.current.stop());
    expect(bridge.stopSharing).toHaveBeenCalled();
    expect(result.current.view).toBeNull();
  });
});
