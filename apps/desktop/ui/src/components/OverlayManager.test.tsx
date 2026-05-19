import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import OverlayProvider, { useOverlay } from "./OverlayManager";

type TestOverlayProps = {
  onResolve?: (value?: string | null) => void;
};

afterEach(() => {
  cleanup();
  document.body.style.overflow = "";
});

describe("OverlayManager", () => {
  it("locks body scroll, resolves the overlay result, and restores trigger focus", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });

    try {
      render(
        <OverlayProvider>
          <OverlayHarness />
        </OverlayProvider>,
      );

      const trigger = screen.getByRole("button", { name: "Open overlay" });

      trigger.focus();
      fireEvent.click(trigger);

      expect(document.body.style.overflow).toBe("hidden");

      fireEvent.click(
        await screen.findByRole("button", { name: "Confirm selection" }),
      );

      await waitFor(() => {
        expect(screen.getByLabelText("overlay result")).toHaveTextContent(
          "target-42",
        );
      });

      await waitFor(() => {
        expect(document.body.style.overflow).toBe("");
      });

      await waitFor(() => {
        expect(trigger).toHaveFocus();
      });
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("resolves null when the overlay cancels", async () => {
    render(
      <OverlayProvider>
        <OverlayHarness />
      </OverlayProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Open overlay" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Cancel selection" }),
    );

    await waitFor(() => {
      expect(screen.getByLabelText("overlay result")).toHaveTextContent("null");
    });
  });
});

function OverlayHarness() {
  const { openOverlay } = useOverlay();
  const [result, setResult] = useState("pending");

  return (
    <>
      <button
        onClick={async () => {
          const nextResult = await openOverlay<string>(TestOverlay);
          setResult(nextResult ?? "null");
        }}
        type="button"
      >
        Open overlay
      </button>

      <output aria-label="overlay result">{result}</output>
    </>
  );
}

function TestOverlay({ onResolve }: TestOverlayProps) {
  return (
    <div aria-label="Test overlay" role="dialog">
      <button onClick={() => onResolve?.("target-42")} type="button">
        Confirm selection
      </button>
      <button onClick={() => onResolve?.(null)} type="button">
        Cancel selection
      </button>
    </div>
  );
}
