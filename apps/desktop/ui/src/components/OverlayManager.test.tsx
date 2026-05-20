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
  label?: string;
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

  it("dismisses stacked overlays in last-in-first-out order", async () => {
    render(
      <OverlayProvider>
        <StackedOverlayHarness />
      </OverlayProvider>,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Open stacked overlays" }),
    );

    expect(
      await screen.findByRole("dialog", { name: "Second overlay" }),
    ).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "Dismiss top overlay" }),
    );

    await waitFor(() => {
      expect(screen.getByLabelText("overlay result second")).toHaveTextContent(
        "null",
      );
    });

    expect(
      screen.queryByRole("dialog", { name: "Second overlay" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("dialog", { name: "First overlay" }),
    ).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "Dismiss top overlay" }),
    );

    await waitFor(() => {
      expect(screen.getByLabelText("overlay result first")).toHaveTextContent(
        "null",
      );
    });

    expect(
      screen.queryByRole("dialog", { name: "First overlay" }),
    ).not.toBeInTheDocument();
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

function StackedOverlayHarness() {
  const { dismissTopOverlay, openOverlay } = useOverlay();
  const [firstResult, setFirstResult] = useState("pending");
  const [secondResult, setSecondResult] = useState("pending");

  return (
    <>
      <button
        onClick={() => {
          void openOverlay<string>(TestOverlay, {
            label: "First overlay",
          }).then((result) => {
            setFirstResult(result ?? "null");
          });
          void openOverlay<string>(TestOverlay, {
            label: "Second overlay",
          }).then((result) => {
            setSecondResult(result ?? "null");
          });
        }}
        type="button"
      >
        Open stacked overlays
      </button>

      <button onClick={() => dismissTopOverlay()} type="button">
        Dismiss top overlay
      </button>

      <output aria-label="overlay result first">{firstResult}</output>
      <output aria-label="overlay result second">{secondResult}</output>
    </>
  );
}

function TestOverlay({ label = "Test overlay", onResolve }: TestOverlayProps) {
  return (
    <div aria-label={label} role="dialog">
      <button onClick={() => onResolve?.("target-42")} type="button">
        Confirm selection
      </button>
      <button onClick={() => onResolve?.(null)} type="button">
        Cancel selection
      </button>
    </div>
  );
}
