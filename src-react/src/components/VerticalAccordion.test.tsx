import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { VerticalAccordion } from "./VerticalAccordion";

describe("VerticalAccordion", () => {
  it("hides the body when collapsed and shows it when expanded", () => {
    const { container } = render(
      <VerticalAccordion
        collapsedToggleLabel="Expand build target"
        expandedToggleLabel="Collapse build target"
        header={<div>Build target</div>}
      >
        <div>Target content</div>
      </VerticalAccordion>,
    );

    const body = container.querySelector(".vertical-accordion__body");
    const icon = container.querySelector(
      ".vertical-accordion__toggle .ui-button__icon",
    );

    expect(body).not.toBeNull();
    expect(icon).not.toBeNull();
    expect(body).toHaveAttribute("aria-hidden", "true");
    expect(body).toHaveStyle({ display: "none" });
    expect(icon).toHaveStyle({ transform: "rotate(-90deg)" });

    fireEvent.click(
      screen.getByRole("button", { name: "Expand build target" }),
    );

    expect(body).toHaveAttribute("aria-hidden", "false");
    expect(body).toHaveStyle({ display: "grid" });
    expect(icon).toHaveStyle({ transform: "rotate(0deg)" });

    fireEvent.click(
      screen.getByRole("button", { name: "Collapse build target" }),
    );

    expect(body).toHaveAttribute("aria-hidden", "true");
    expect(body).toHaveStyle({ display: "none" });
    expect(icon).toHaveStyle({ transform: "rotate(-90deg)" });
  });

  it("toggles the body from the interactive header with Enter and Space", () => {
    const { container } = render(
      <VerticalAccordion
        collapsedToggleLabel="Expand build target"
        expandedToggleLabel="Collapse build target"
        header={<div>Build target</div>}
        triggerMode="header"
      >
        <div>Target content</div>
      </VerticalAccordion>,
    );

    const header = container.querySelector(
      ".vertical-accordion__header[role='button']",
    );
    const body = container.querySelector(".vertical-accordion__body");

    expect(header).not.toBeNull();
    expect(body).toHaveAttribute("aria-hidden", "true");

    fireEvent.keyDown(header!, { key: "Enter" });
    expect(body).toHaveAttribute("aria-hidden", "false");
    expect(body).toHaveStyle({ display: "grid" });

    fireEvent.keyDown(header!, { key: " " });
    expect(body).toHaveAttribute("aria-hidden", "true");
    expect(body).toHaveStyle({ display: "none" });
  });
});
