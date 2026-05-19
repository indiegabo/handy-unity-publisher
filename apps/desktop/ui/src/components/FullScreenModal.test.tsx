import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import FullScreenModal from "./FullScreenModal";

afterEach(() => {
  cleanup();
});

describe("FullScreenModal", () => {
  it("prioritizes the explicit autofocus target instead of the close button", async () => {
    render(
      <FullScreenModal
        description="Search before selecting a project."
        onResolve={() => undefined}
        title="Open project"
      >
        <input aria-label="Filter inventory" data-overlay-autofocus />
        <button type="button">Secondary action</button>
      </FullScreenModal>,
    );

    await waitFor(() => {
      expect(
        screen.getByRole("textbox", { name: "Filter inventory" }),
      ).toHaveFocus();
    });
  });

  it("wraps focus inside the modal and resolves null on Escape", () => {
    const onResolve = vi.fn();

    render(
      <FullScreenModal onResolve={onResolve} title="Open project">
        <input aria-label="Filter inventory" data-overlay-autofocus />
        <button type="button">Secondary action</button>
      </FullScreenModal>,
    );

    const dialog = screen.getByRole("dialog");
    const closeButton = screen.getByRole("button", { name: "Close overlay" });
    const secondaryButton = screen.getByRole("button", {
      name: "Secondary action",
    });

    secondaryButton.focus();
    fireEvent.keyDown(secondaryButton, { key: "Tab" });
    expect(closeButton).toHaveFocus();

    closeButton.focus();
    fireEvent.keyDown(closeButton, { key: "Tab", shiftKey: true });
    expect(secondaryButton).toHaveFocus();

    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(onResolve).toHaveBeenCalledWith(null);
  });
});
