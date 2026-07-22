import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

afterEach(() => {
  cleanup();
  localStorage.clear();
  window.history.replaceState({}, "", "/");
  vi.resetModules();
});

describe("Ket desktop shell", () => {
  it("retains a successful profile and access expiry", async () => {
    window.history.replaceState({}, "", "/");
    const { default: App } = await import("./App");
    render(<App />);

    expect(await screen.findByRole("heading", { name: "Choose your node" })).toBeInTheDocument();
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

    const { default: App } = await import("./App");
    render(<App />);

    expect(await screen.findByLabelText("Server URL")).toHaveValue("");
    expect(screen.getByLabelText("Access code")).toHaveValue("");
    expect(localStorage.getItem("ket.server-url")).toBeNull();
    expect(localStorage.getItem("ket.access-code")).toBeNull();
    expect(localStorage.getItem("ket.access-expires-at")).toBeNull();
  });

  it("prefills an unexpired profile and shows its remaining access", async () => {
    const expiry = Math.floor(Date.now() / 1_000) + 3_600;
    localStorage.setItem("ket.server-url", "https://saved.example.test");
    localStorage.setItem("ket.access-code", "S".repeat(32));
    localStorage.setItem("ket.access-expires-at", expiry.toString());

    const { default: App } = await import("./App");
    render(<App />);

    expect(await screen.findByLabelText("Server URL")).toHaveValue("https://saved.example.test");
    expect(screen.getByLabelText("Access code")).toHaveValue("S".repeat(32));
    expect(screen.getByText(/access left/)).toBeInTheDocument();
  });

  it("shows connected telemetry and navigates to metrics", async () => {
    window.history.replaceState({}, "", "/?demo=connected");
    const { default: App } = await import("./App");
    render(<App />);

    expect(await screen.findByRole("status")).toHaveTextContent("Liberated");
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

    fireEvent.change(await screen.findByLabelText("Server URL"), {
      target: { value: "https://de-fra.ket.example" },
    });
    fireEvent.change(screen.getByLabelText("Access code"), {
      target: { value: "A2345678901234567890123456789012" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add server" }));

    const selector = await screen.findByLabelText("Preferred protocol");
    fireEvent.change(selector, { target: { value: "stealth" } });
    expect(localStorage.getItem("ket.protocol-preference")).toBe("stealth");

    fireEvent.click(screen.getByRole("button", { name: "Learn more" }));
    expect(screen.getByRole("heading", { name: "HTTPS Stealth", level: 1 })).toBeInTheDocument();
    expect(screen.getByText(/XHTTP packet-up/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Connection" }));
    fireEvent.click(screen.getByRole("button", { name: "Connect tunnel" }));
    expect(await screen.findByText("Restricted-network bypass active")).toBeInTheDocument();
    expect(screen.getAllByText("HTTPS Stealth").length).toBeGreaterThan(0);
  });
});
