import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import InputWithPicker from "./InputWithPicker";
import OverlayProvider from "./OverlayManager";

afterEach(() => {
  cleanup();
});

describe("InputWithPicker", () => {
  it("exposes the text input with its visible label as the accessible name", () => {
    render(
      <OverlayProvider>
        <InputWithPicker
          buttonLabel="Browse"
          hint="2 matching projects"
          label="Quick open"
          value=""
        />
      </OverlayProvider>,
    );

    expect(screen.getByRole("textbox", { name: "Quick open" })).toHaveValue("");
  });

  it("forwards the overlay result to onPick when a picker component resolves", async () => {
    const onPick = vi.fn();

    render(
      <OverlayProvider>
        <InputWithPicker
          buttonLabel="Browse"
          label="Quick open"
          onPick={onPick}
          pickerComponent={TestPickerOverlay}
          pickerProps={{ resolvedValue: "repository-7" }}
          value="Worker Demo"
        />
      </OverlayProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Browse" }));

    fireEvent.click(
      await screen.findByRole("button", { name: "Choose repository-7" }),
    );

    await waitFor(() => {
      expect(onPick).toHaveBeenCalledWith("repository-7");
    });
  });

  it("falls back to onChange when no explicit onPick handler is provided", async () => {
    const onChange = vi.fn();

    render(
      <OverlayProvider>
        <InputWithPicker
          buttonLabel="Browse"
          label="Quick open"
          onChange={onChange}
          pickerComponent={TestPickerOverlay}
          pickerProps={{ resolvedValue: "repository-9" }}
          value=""
        />
      </OverlayProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Browse" }));

    fireEvent.click(
      await screen.findByRole("button", { name: "Choose repository-9" }),
    );

    await waitFor(() => {
      expect(onChange).toHaveBeenCalledWith("repository-9");
    });
  });
});

function TestPickerOverlay({
  onResolve,
  resolvedValue,
}: {
  onResolve?: (value?: string | null) => void;
  resolvedValue: string;
}) {
  return (
    <div aria-label="Test picker overlay" role="dialog">
      <button onClick={() => onResolve?.(resolvedValue)} type="button">
        {`Choose ${resolvedValue}`}
      </button>
    </div>
  );
}
