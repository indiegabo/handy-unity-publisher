import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { LogViewerModal } from "./LogViewerModal";

afterEach(() => {
  cleanup();
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: undefined,
  });
});

beforeEach(() => {
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: {
      writeText: vi.fn().mockResolvedValue(undefined),
    },
  });
});

describe("LogViewerModal", () => {
  it("autofocuses the wrap toggle and copies the current content", async () => {
    render(
      <LogViewerModal
        content="Build completed successfully."
        meta="Showing the full log file."
        title="Editor.log"
      />,
    );

    const wrapToggle = screen.getByRole("button", { name: "Preserve lines" });

    await waitFor(() => {
      expect(wrapToggle).toHaveFocus();
    });

    fireEvent.click(screen.getByRole("button", { name: "Copy content" }));

    await waitFor(() => {
      expect(navigator.clipboard.writeText).toHaveBeenCalledWith(
        "Build completed successfully.",
      );
    });

    expect(
      screen.getByText("Copied the current viewer content to the clipboard."),
    ).toBeInTheDocument();
  });
});
