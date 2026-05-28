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
  vi.restoreAllMocks();
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
    const { container } = render(
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

    expect(
      container.querySelector(".log-viewer-modal__summary-strip.ui-summary-strip"),
    ).not.toBeNull();

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

  it("downloads the current content with the provided file name", async () => {
    const createObjectURLMock = vi.fn(() => "blob:test-log");
    const revokeObjectURLMock = vi.fn();
    const originalCreateElement = document.createElement.bind(document);
    const anchorClickMock = vi.fn();
    const createdAnchorRef: { current: HTMLAnchorElement | null } = {
      current: null,
    };

    Object.defineProperty(URL, "createObjectURL", {
      configurable: true,
      value: createObjectURLMock,
    });
    Object.defineProperty(URL, "revokeObjectURL", {
      configurable: true,
      value: revokeObjectURLMock,
    });

    vi.spyOn(document, "createElement").mockImplementation((tagName: string) => {
      const element = originalCreateElement(tagName);

      if (tagName === "a") {
        createdAnchorRef.current = element as HTMLAnchorElement;
        Object.defineProperty(createdAnchorRef.current, "click", {
          configurable: true,
          value: anchorClickMock,
        });
      }

      return element;
    });

    render(
      <LogViewerModal
        content="Build completed successfully."
        downloadFileName="Editor.log"
        title="Editor.log"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Download content" }));

    await waitFor(() => {
      expect(createObjectURLMock).toHaveBeenCalledTimes(1);
      expect(anchorClickMock).toHaveBeenCalledTimes(1);
      expect(revokeObjectURLMock).toHaveBeenCalledWith("blob:test-log");
    });

    if (!createdAnchorRef.current) {
      throw new Error("The download action did not create an anchor element.");
    }

    const anchor = createdAnchorRef.current;

    expect(anchor.download).toBe("Editor.log");
    expect(anchor.href).toBe("blob:test-log");
    expect(screen.getByText("Downloaded Editor.log.")).toBeInTheDocument();
  });
});
