import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { testSnapshot } from "../test/fixtures";
import type { TransportFallback } from "../types";
import { FallbackNote } from "./FallbackNote";

const realityBlocked: TransportFallback = {
  transport: {
    id: "reality-primary",
    display_name: "VLESS + REALITY",
    protocol: "vless_xtls_reality",
    network: "tcp",
  },
  issue: {
    code: "transport_failed",
    message: "the TLS handshake timed out",
    retryable: true,
  },
};

describe("FallbackNote", () => {
  it("says nothing when the first protocol worked", () => {
    const { container } = render(<FallbackNote snapshot={testSnapshot()} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("names the protocol in use and why the others were left", () => {
    render(
      <FallbackNote
        snapshot={testSnapshot({
          phase: "connected",
          active_transport: {
            id: "stealth-primary",
            display_name: "HTTPS Stealth",
            protocol: "stealth",
            network: "tcp",
          },
          fallbacks: [realityBlocked],
        })}
      />,
    );
    const note = screen.getByRole("status", { name: "Why Ket switched protocols" });
    expect(note).toHaveTextContent("Using HTTPS Stealth because:");
    expect(note).toHaveTextContent("VLESS + REALITY: the TLS handshake timed out");
  });

  it("lists every attempt when none connected", () => {
    render(
      <FallbackNote
        snapshot={testSnapshot({
          phase: "error",
          fallbacks: [
            realityBlocked,
            {
              ...realityBlocked,
              transport: { ...realityBlocked.transport, id: "stealth", display_name: "HTTPS Stealth" },
              issue: { ...realityBlocked.issue, message: "connection refused" },
            },
          ],
        })}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("Tried:");
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    expect(screen.getByText(/connection refused/)).toBeInTheDocument();
  });
});
