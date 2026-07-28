import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DiagnosticsDialog } from "./DiagnosticsDialog";

afterEach(() => vi.restoreAllMocks());

describe("diagnostics dialog", () => {
  it("renders nothing while closed and exposes a modal without secrets when open", () => {
    const { rerender } = render(
      <DiagnosticsDialog open={false} report="safe report" onClose={vi.fn()} />,
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    rerender(<DiagnosticsDialog open report="safe report" onClose={vi.fn()} />);
    expect(screen.getByRole("dialog", { name: "Connection diagnostics" })).toBeInTheDocument();
    expect(screen.getByText("safe report")).toBeInTheDocument();
    expect(screen.getByText(/credentials are excluded/i)).toBeInTheDocument();
  });

  it("keeps actions outside the scrollable report region", () => {
    render(<DiagnosticsDialog open report={"failure\n".repeat(500)} onClose={vi.fn()} />);

    const dialog = screen.getByRole("dialog", { name: "Connection diagnostics" });
    const body = dialog.querySelector(".diagnostics-body");
    const footer = dialog.querySelector("footer");
    const report = screen.getByRole("region", { name: "Connection diagnostic report" });
    const copyButton = screen.getByRole("button", { name: "Copy logs" });

    expect(body).not.toBeNull();
    expect(footer).not.toBeNull();
    expect(body).toContainElement(report);
    expect(body).not.toContainElement(copyButton);
    expect(footer).toContainElement(copyButton);
    expect(copyButton).toBeVisible();
    expect(report).toHaveAttribute("tabindex", "0");
    expect(Array.from(dialog.children)).toEqual([
      dialog.querySelector("header"),
      body,
      footer,
    ]);
  });

  it("copies the exact report and resets copied state after closing", async () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    const report = "line 1\nline 2";
    const { rerender } = render(
      <DiagnosticsDialog open report={report} onClose={vi.fn()} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Copy logs" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(report));
    expect(screen.getByRole("button", { name: "Copied" })).toBeInTheDocument();

    rerender(<DiagnosticsDialog open={false} report={report} onClose={vi.fn()} />);
    rerender(<DiagnosticsDialog open report={report} onClose={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Copy logs" })).toBeInTheDocument();
  });

  it("reports clipboard denial without an unhandled rejection", async () => {
    const writeText = vi.fn(async () => {
      throw new DOMException("Permission denied", "NotAllowedError");
    });
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    render(<DiagnosticsDialog open report="safe report" onClose={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "Copy logs" }));

    expect(await screen.findByRole("button", { name: "Copy failed" })).toBeInTheDocument();
  });

  it("closes from escape, the close button, and backdrop but not dialog content", () => {
    const onClose = vi.fn();
    const { container } = render(
      <DiagnosticsDialog open report="report" onClose={onClose} />,
    );
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(screen.getByRole("button", { name: "Close diagnostics" }));
    fireEvent.mouseDown(screen.getByRole("dialog"));
    expect(onClose).toHaveBeenCalledTimes(2);
    fireEvent.mouseDown(container.querySelector(".diagnostics-backdrop") as Element);
    expect(onClose).toHaveBeenCalledTimes(3);
  });
});
