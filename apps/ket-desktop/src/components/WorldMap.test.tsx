import { render, screen } from "@testing-library/react";
import { expect, it } from "vitest";
import { WorldMap } from "./WorldMap";

const singapore = {
  country_code: "SG",
  country_name: "Singapore",
  city: "Singapore",
  latitude: 1.29,
  longitude: 103.85,
};

it("keeps the world view and a restrained marker while disconnected", () => {
  const { container } = render(<WorldMap location={singapore} connected={false} />);

  expect(screen.getByRole("img", { name: /Server location: Singapore/ })).toBeInTheDocument();
  expect(container.querySelector(".map-viewport")).toHaveAttribute(
    "transform",
    "translate(0 0) scale(1)",
  );
  expect(container.querySelector(".map-marker-ring")).toHaveAttribute("r", "10");
});

it("focuses further into the active region only after connection", () => {
  const { container } = render(<WorldMap location={singapore} connected />);

  expect(container.querySelector(".world-map")).toHaveClass("is-connected");
  expect(container.querySelector(".map-viewport")).toHaveAttribute(
    "transform",
    expect.stringContaining("scale(2.45)"),
  );
  expect(container.querySelector(".map-marker-ring")).toHaveAttribute("r", "18");
});
