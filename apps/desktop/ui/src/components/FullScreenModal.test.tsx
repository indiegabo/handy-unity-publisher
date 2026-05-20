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

  it("focuses an explicit autofocus target with tabindex -1 before interactive controls", async () => {
    render(
      <FullScreenModal onResolve={() => undefined} title="Project workers">
        <p data-overlay-autofocus tabIndex={-1}>
          Loading project worker inventory...
        </p>
        <button type="button">Open Project Workers</button>
      </FullScreenModal>,
    );

    await waitFor(() => {
      expect(
        screen.getByText("Loading project worker inventory..."),
      ).toHaveFocus();
    });
  });

  it("ignores data-overlay-autofocus set to false and uses the next valid target", async () => {
    render(
      <FullScreenModal onResolve={() => undefined} title="Artifact viewer">
        <button data-overlay-autofocus={false} type="button">
          Open artifact
        </button>
        <button data-overlay-autofocus type="button">
          Open folder
        </button>
      </FullScreenModal>,
    );

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Open folder" })).toHaveFocus();
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

  it("falls back to the first focusable control when the annotated target is disabled", async () => {
    render(
      <FullScreenModal onResolve={() => undefined} title="Artifact viewer">
        <button data-overlay-autofocus disabled type="button">
          Open artifact
        </button>
        <button type="button">Open folder</button>
      </FullScreenModal>,
    );

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Close overlay" }),
      ).toHaveFocus();
    });
  });
});
