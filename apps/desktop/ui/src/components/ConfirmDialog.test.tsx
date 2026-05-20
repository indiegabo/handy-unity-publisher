import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ConfirmDialog } from "./ConfirmDialog";

afterEach(() => {
  cleanup();
});

describe("ConfirmDialog", () => {
  it("autofocuses the cancel action and resolves null on Escape", async () => {
    const onResolve = vi.fn();

    render(
      <ConfirmDialog
        description="Keep the operator in the current shell context."
        message="The current release run will remain untouched."
        onResolve={onResolve}
        title="Delete release run?"
      />,
    );

    const cancelButton = screen.getByRole("button", { name: "Stay in shell" });

    await waitFor(() => {
      expect(cancelButton).toHaveFocus();
    });

    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });

    expect(onResolve).toHaveBeenCalledWith(null);
  });

  it("resolves true when the confirm action is pressed", () => {
    const onResolve = vi.fn();

    render(
      <ConfirmDialog
        confirmLabel="Delete run"
        confirmVariant="secondary"
        onResolve={onResolve}
        title="Delete release run?"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Delete run" }));

    expect(onResolve).toHaveBeenCalledWith(true);
  });
});
