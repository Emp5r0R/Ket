import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { EnrollmentPanel } from "./EnrollmentPanel";

afterEach(() => {
  vi.useRealTimers();
});

describe("server enrollment", () => {
  it("sanitizes and hides the access code, then clears it before enrollment", async () => {
    const onEnroll = vi.fn(async () => undefined);
    render(
      <EnrollmentPanel
        initialServerUrl="https://ket.example.test"
        initialAccessCode=""
        accessExpiresAtEpochSeconds={null}
        initialDeviceName="Workstation"
        busy={false}
        issue={null}
        onEnroll={onEnroll}
      />,
    );
    const input = screen.getByLabelText("Access code") as HTMLInputElement;
    expect(input).toHaveAttribute("type", "password");
    fireEvent.change(input, { target: { value: `AAAA AAAA AAAA AAAA AAAA AAAA AAAA AAAA` } });
    expect(input).toHaveValue("A".repeat(32));
    expect(screen.getByText("32/32")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Show access code" }));
    expect(input).toHaveAttribute("type", "text");
    fireEvent.click(screen.getByRole("button", { name: "Hide access code" }));
    expect(input).toHaveAttribute("type", "password");

    fireEvent.click(screen.getByRole("button", { name: "Add server" }));
    await waitFor(() => expect(onEnroll).toHaveBeenCalledOnce());
    expect(onEnroll).toHaveBeenCalledWith({
      serverUrl: "https://ket.example.test",
      deviceName: "Workstation",
      accessCode: "A".repeat(32),
    });
    expect(input).toHaveValue("");
  });

  it("enforces exact code length and disables all fields while busy", () => {
    const { rerender } = render(
      <EnrollmentPanel
        initialServerUrl=""
        initialAccessCode={"A".repeat(31)}
        accessExpiresAtEpochSeconds={null}
        initialDeviceName="Ket desktop"
        busy={false}
        issue={null}
        onEnroll={vi.fn(async () => undefined)}
      />,
    );
    expect(screen.getByRole("button", { name: "Add server" })).toBeDisabled();

    rerender(
      <EnrollmentPanel
        initialServerUrl="https://ket.example.test"
        initialAccessCode={"A".repeat(32)}
        accessExpiresAtEpochSeconds={null}
        initialDeviceName="Ket desktop"
        busy
        issue={null}
        onEnroll={vi.fn(async () => undefined)}
      />,
    );
    expect(screen.getByLabelText("Server URL")).toBeDisabled();
    expect(screen.getByLabelText("Access code")).toBeDisabled();
    expect(screen.getByLabelText("Device name")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Add server" })).toBeDisabled();
  });

  it("shows bounded access lifetime and enrollment failures", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-26T12:00:00Z"));
    const now = Math.floor(Date.now() / 1_000);
    render(
      <EnrollmentPanel
        initialServerUrl="https://ket.example.test"
        initialAccessCode={"A".repeat(32)}
        accessExpiresAtEpochSeconds={now + 3_661}
        initialDeviceName="Ket desktop"
        busy={false}
        issue={{ code: "grant_expired", message: "Access code expired", retryable: false }}
        onEnroll={vi.fn(async () => undefined)}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("1h 1m access left");
    expect(screen.getByRole("alert")).toHaveTextContent("Access code expired");
  });
});
