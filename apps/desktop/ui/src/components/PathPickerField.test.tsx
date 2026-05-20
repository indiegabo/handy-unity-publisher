import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import OverlayProvider from "./OverlayManager";
import { PathPickerField } from "./PathPickerField";

const { pickHostPathMock } = vi.hoisted(() => ({
  pickHostPathMock: vi.fn(),
}));

vi.mock("../services/projects", () => ({
  pickHostPath: pickHostPathMock,
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

beforeEach(() => {
  pickHostPathMock.mockReset();
});

describe("PathPickerField", () => {
  it("uses the native picker result when the host picker succeeds", async () => {
    const onPathPicked = vi.fn();

    pickHostPathMock.mockResolvedValueOnce("C:/Projects/HGP");

    render(
      <OverlayProvider>
        <PathPickerField
          buttonLabel="Choose artifacts root"
          dialogTitle="Choose artifacts root"
          label="Artifacts root"
          onPathPicked={onPathPicked}
          pickerKind="directory"
          value=""
        />
      </OverlayProvider>,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Choose artifacts root" }),
    );

    await waitFor(() => {
      expect(pickHostPathMock).toHaveBeenCalledWith({
        kind: "directory",
        title: "Choose artifacts root",
      });
    });

    await waitFor(() => {
      expect(onPathPicked).toHaveBeenCalledWith("C:/Projects/HGP");
    });

    expect(
      screen.queryByRole("dialog", { name: "Enter path manually" }),
    ).not.toBeInTheDocument();
  });

  it("does not open the fallback overlay when the native picker is cancelled", async () => {
    const onPathPicked = vi.fn();

    pickHostPathMock.mockResolvedValueOnce(null);

    render(
      <OverlayProvider>
        <PathPickerField
          buttonLabel="Choose workspace root"
          label="Workspace root"
          onPathPicked={onPathPicked}
          pickerKind="directory"
          value="C:/Existing"
        />
      </OverlayProvider>,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Choose workspace root" }),
    );

    await waitFor(() => {
      expect(pickHostPathMock).toHaveBeenCalledWith({ kind: "directory" });
    });

    expect(onPathPicked).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("dialog", { name: "Enter path manually" }),
    ).not.toBeInTheDocument();
  });

  it("falls back to the manual path overlay when the native picker fails", async () => {
    const onPathPicked = vi.fn();

    pickHostPathMock.mockRejectedValueOnce(
      new Error("native picker unavailable"),
    );

    render(
      <OverlayProvider>
        <PathPickerField
          buttonLabel="Choose Unity executable"
          label="Unity executable"
          onPathPicked={onPathPicked}
          pickerKind="file"
          value="C:/Existing/Unity.exe"
        />
      </OverlayProvider>,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "Choose Unity executable" }),
    );

    const dialog = await screen.findByRole("dialog", {
      name: "Enter path manually",
    });
    const pathInput = within(dialog).getByDisplayValue("C:/Existing/Unity.exe");

    expect(pathInput).toHaveValue("C:/Existing/Unity.exe");

    fireEvent.change(pathInput, {
      target: { value: "  C:/Manual/Unity.exe  " },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Use path" }));

    await waitFor(() => {
      expect(onPathPicked).toHaveBeenCalledWith("C:/Manual/Unity.exe");
    });
  });
});
