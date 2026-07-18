import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

afterEach(() => {
  cleanup();
  localStorage.clear();
  window.history.replaceState({}, "", "/");
  vi.resetModules();
});

describe("Ket desktop shell", () => {
  it("enrolls from the map-first empty state without retaining the access code", async () => {
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
  });

  it("shows connected telemetry and navigates to metrics", async () => {
    window.history.replaceState({}, "", "/?demo=connected");
    const { default: App } = await import("./App");
    render(<App />);

    expect(await screen.findByText("Protected")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /Server location: Frankfurt/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Metrics" }));
    expect(screen.getByRole("heading", { name: "Network metrics" })).toBeInTheDocument();
    expect(screen.getByText("23%")).toBeInTheDocument();
    expect(screen.getByText("Hysteria 2")).toBeInTheDocument();
    expect(screen.getByText("42 ms")).toBeInTheDocument();
  });
});
