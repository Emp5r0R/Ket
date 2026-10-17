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
        initialMode="manual"
        busy={false}
        issue={null}
        onEnroll={onEnroll}
        onEnrollInvite={vi.fn(async () => undefined)}
        onEnrollInviteImage={vi.fn(async () => undefined)}
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
      nodeKey: null,
      sealedControl: false,
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
        initialMode="manual"
        busy={false}
        issue={null}
        onEnroll={vi.fn(async () => undefined)}
        onEnrollInvite={vi.fn(async () => undefined)}
        onEnrollInviteImage={vi.fn(async () => undefined)}
      />,
    );
    expect(screen.getByRole("button", { name: "Add server" })).toBeDisabled();

    rerender(
      <EnrollmentPanel
        initialServerUrl="https://ket.example.test"
        initialAccessCode={"A".repeat(32)}
        accessExpiresAtEpochSeconds={null}
        initialDeviceName="Ket desktop"
        initialMode="manual"
        busy
        issue={null}
        onEnroll={vi.fn(async () => undefined)}
        onEnrollInvite={vi.fn(async () => undefined)}
        onEnrollInviteImage={vi.fn(async () => undefined)}
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
        onEnrollInvite={vi.fn(async () => undefined)}
        onEnrollInviteImage={vi.fn(async () => undefined)}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("1h 1m access left");
    expect(screen.getByRole("alert")).toHaveTextContent("Access code expired");
  });

  it("defaults to a masked invite and hands it over once, then clears it", async () => {
    const onEnroll = vi.fn(async () => undefined);
    const onEnrollInvite = vi.fn(async () => undefined);
    render(
      <EnrollmentPanel
        initialServerUrl=""
        initialAccessCode=""
        accessExpiresAtEpochSeconds={null}
        initialDeviceName="Laptop"
        busy={false}
        issue={null}
        onEnroll={onEnroll}
        onEnrollInvite={onEnrollInvite}
        onEnrollInviteImage={vi.fn(async () => undefined)}
      />,
    );
    expect(screen.queryByLabelText("Server URL")).not.toBeInTheDocument();
    const invite = screen.getByLabelText("Invite") as HTMLInputElement;
    expect(invite).toHaveAttribute("type", "password");
    expect(screen.getByRole("button", { name: "Add server" })).toBeDisabled();

    const text = `ket:invite:${"Ab_-".repeat(40)}`;
    fireEvent.change(invite, { target: { value: ` ${text.slice(0, 50)}\n${text.slice(50)} ` } });
    expect(invite).toHaveValue(text);
    fireEvent.click(screen.getByRole("button", { name: "Show invite" }));
    expect(invite).toHaveAttribute("type", "text");

    fireEvent.click(screen.getByRole("button", { name: "Add server" }));
    await waitFor(() => expect(onEnrollInvite).toHaveBeenCalledOnce());
    expect(onEnrollInvite).toHaveBeenCalledWith(text, "Laptop");
    expect(onEnroll).not.toHaveBeenCalled();
    expect(invite).toHaveValue("");
  });

  it("switches between invite and advanced URL entry", () => {
    render(
      <EnrollmentPanel
        initialServerUrl=""
        initialAccessCode=""
        accessExpiresAtEpochSeconds={null}
        initialDeviceName="Laptop"
        busy={false}
        issue={null}
        onEnroll={vi.fn(async () => undefined)}
        onEnrollInvite={vi.fn(async () => undefined)}
        onEnrollInviteImage={vi.fn(async () => undefined)}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Advanced: use a server URL and access code" }));
    expect(screen.getByLabelText("Server URL")).toBeInTheDocument();
    expect(screen.getByLabelText("Access code")).toBeInTheDocument();
    expect(screen.queryByLabelText("Invite")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Use an invite instead" }));
    expect(screen.getByLabelText("Invite")).toBeInTheDocument();
  });

  it("hands a chosen or pasted PNG to native QR decoding and rejects other files", async () => {
    const onEnrollInviteImage = vi.fn(async () => undefined);
    render(
      <EnrollmentPanel
        initialServerUrl=""
        initialAccessCode=""
        accessExpiresAtEpochSeconds={null}
        initialDeviceName="Laptop"
        busy={false}
        issue={null}
        onEnroll={vi.fn(async () => undefined)}
        onEnrollInvite={vi.fn(async () => undefined)}
        onEnrollInviteImage={onEnrollInviteImage}
      />,
    );
    const picker = screen.getByLabelText("QR image file") as HTMLInputElement;
    expect(picker).toHaveAttribute("accept", "image/png,image/jpeg");

    const png = new File([new Uint8Array([137, 80, 78, 71])], "invite.png", { type: "image/png" });
    fireEvent.change(picker, { target: { files: [png] } });
    await waitFor(() => expect(onEnrollInviteImage).toHaveBeenCalledOnce());
    expect(onEnrollInviteImage).toHaveBeenCalledWith(new Uint8Array([137, 80, 78, 71]), "Laptop");

    const gif = new File([new Uint8Array([71, 73, 70])], "invite.gif", { type: "image/gif" });
    fireEvent.change(picker, { target: { files: [gif] } });
    expect(await screen.findByRole("alert")).toHaveTextContent("PNG or JPEG");
    const huge = new File([new Uint8Array(8 * 1024 * 1024 + 1)], "huge.png", { type: "image/png" });
    fireEvent.change(picker, { target: { files: [huge] } });
    expect(onEnrollInviteImage).toHaveBeenCalledOnce();

    fireEvent.paste(screen.getByLabelText("Invite"), { clipboardData: { files: [png] } });
    await waitFor(() => expect(onEnrollInviteImage).toHaveBeenCalledTimes(2));
  });
});
