import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ProtocolMenu } from "./ProtocolMenu";

afterEach(cleanup);

const options = [
  { value: "auto" as const, label: "Automatic" },
  { value: "stealth" as const, label: "HTTPS Stealth" },
  { value: "vless_xtls_reality" as const, label: "VLESS + REALITY" },
];

it("selects a protocol from the themed menu", () => {
  const onChange = vi.fn();
  render(
    <div>
      <label htmlFor="protocol-menu">Preferred protocol</label>
      <ProtocolMenu
        id="protocol-menu"
        value="auto"
        options={options}
        disabled={false}
        onChange={onChange}
      />
    </div>,
  );

  fireEvent.click(screen.getByRole("combobox", { name: "Preferred protocol" }));
  fireEvent.click(screen.getByRole("option", { name: "HTTPS Stealth" }));

  expect(onChange).toHaveBeenCalledWith("stealth");
  expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
});

it("supports keyboard navigation and escape", () => {
  render(
    <div>
      <label htmlFor="protocol-menu">Preferred protocol</label>
      <ProtocolMenu
        id="protocol-menu"
        value="stealth"
        options={options}
        disabled={false}
        onChange={vi.fn()}
      />
    </div>,
  );

  const trigger = screen.getByRole("combobox", { name: "Preferred protocol" });
  fireEvent.keyDown(trigger, { key: "ArrowDown" });
  expect(screen.getByRole("listbox")).toBeInTheDocument();
  fireEvent.keyDown(document, { key: "Escape" });
  expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
});
