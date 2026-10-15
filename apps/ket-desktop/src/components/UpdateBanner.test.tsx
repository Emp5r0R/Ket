import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ClientUpdate } from "../types";
import { UpdateBanner } from "./UpdateBanner";

const update = (status: ClientUpdate["status"]): ClientUpdate => ({
  status,
  latest_version: "1.2.0",
  summary: "Fixes a leak",
  download_url: "https://mirror.example.test/ket",
});

beforeEach(() => window.localStorage.clear());
afterEach(() => vi.restoreAllMocks());

describe("update banner", () => {
  it("renders nothing without advice or for a current app", () => {
    const { container, rerender } = render(<UpdateBanner update={null} />);
    expect(container).toBeEmptyDOMElement();
    rerender(<UpdateBanner update={update("current")} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("lets people hide an optional update until the next version", () => {
    const { container, rerender } = render(<UpdateBanner update={update("optional")} />);
    expect(screen.getByRole("status", { name: "App update" })).toHaveTextContent("Ket 1.2.0 is available");
    fireEvent.click(screen.getByRole("button", { name: "Hide this update" }));
    expect(container).toBeEmptyDOMElement();
    rerender(<UpdateBanner update={{ ...update("optional"), latest_version: "1.3.0" }} />);
    expect(screen.getByRole("status", { name: "App update" })).toHaveTextContent("1.3.0");
  });

  it("keeps a required update on screen as an alert and copies the link", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(<UpdateBanner update={update("required")} />);
    const alert = screen.getByRole("alert", { name: "App update" });
    expect(alert).toHaveTextContent("Update required");
    expect(alert).toHaveTextContent("Fixes a leak");
    expect(screen.queryByRole("button", { name: "Hide this update" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Copy link" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("https://mirror.example.test/ket"));
    expect(screen.getByRole("button", { name: "Copied" })).toBeInTheDocument();
  });

  it("does not offer to hide a security update", () => {
    render(<UpdateBanner update={update("security")} />);
    expect(screen.getByRole("status", { name: "App update" })).toHaveTextContent("Security update");
    expect(screen.queryByRole("button", { name: "Hide this update" })).not.toBeInTheDocument();
  });
});
