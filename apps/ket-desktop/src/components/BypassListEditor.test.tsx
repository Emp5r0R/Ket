import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { BypassListEditor } from "./BypassListEditor";

describe("bypass list editor", () => {
  it("adds the checked form of an entry and warns what bypassing means", async () => {
    const onChange = vi.fn();
    const check = vi.fn(async () => "203.0.113.0/24");
    render(<BypassListEditor entries={["10.0.0.0/8"]} check={check} onChange={onChange} />);

    expect(screen.getByText(/anyone watching it, can see that traffic/i)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText(/address or network to keep outside/i), {
      target: { value: " 203.0.113.9/24 " },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));

    await waitFor(() => expect(onChange).toHaveBeenCalledWith(["10.0.0.0/8", "203.0.113.0/24"]));
    expect(check).toHaveBeenCalledWith(" 203.0.113.9/24 ");
  });

  it("explains a refused entry and keeps the list unchanged", async () => {
    const onChange = vi.fn();
    const check = vi.fn(async () => {
      throw { code: "bypass_too_wide", message: "the network is wider than /8", retryable: false };
    });
    render(<BypassListEditor entries={[]} check={check} onChange={onChange} />);

    fireEvent.change(screen.getByLabelText(/address or network to keep outside/i), {
      target: { value: "0.0.0.0/0" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(/too large/i);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("removes an entry", () => {
    const onChange = vi.fn();
    render(
      <BypassListEditor entries={["10.0.0.0/8", "2001:db8::/32"]} check={vi.fn()} onChange={onChange} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Stop keeping 10.0.0.0/8 outside the tunnel" }));
    expect(onChange).toHaveBeenCalledWith(["2001:db8::/32"]);
  });

  it("stops accepting entries when the list is full", () => {
    const entries = Array.from({ length: 64 }, (_, index) => `10.0.${index}.0/24`);
    render(<BypassListEditor entries={entries} check={vi.fn()} onChange={vi.fn()} />);
    expect(screen.getByLabelText(/address or network to keep outside/i)).toBeDisabled();
    expect(screen.getByText(/the list is full/i)).toBeInTheDocument();
  });
});
