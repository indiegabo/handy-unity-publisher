import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import SelectListFullScreen from "./SelectListFullScreen";
import OverlayProvider, { useOverlay } from "./OverlayManager";

afterEach(() => {
  cleanup();
});

describe("SelectListFullScreen", () => {
  it("uses a shared summary strip and resolves batched selections", async () => {
    const onResolve = vi.fn();

    render(
      <OverlayProvider>
        <SelectListFullScreen
          initialValues={["worker-demo"]}
          items={[
            {
              id: "worker-demo",
              label: "Worker Demo",
              subtitle: "Active repository",
            },
            {
              id: "build-lab",
              label: "Build Lab",
              subtitle: "Disabled repository",
            },
          ]}
          onResolve={onResolve}
          selectionMode="multiple"
          selectionLabel="Selected"
          submitLabel="Apply projects"
          title="Select project"
        />
      </OverlayProvider>,
    );

    const dialog = await screen.findByRole("dialog", {
      name: "Select project",
    });

    expect(
      dialog.querySelector(".select-list-modal__summary-strip"),
    ).not.toBeNull();

    fireEvent.change(screen.getByRole("textbox", { name: "Filter inventory" }), {
      target: { value: "build" },
    });

    fireEvent.click(
      within(dialog).getByRole("button", {
        name: /Build Lab.*Disabled repository.*Select/i,
      }),
    );

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Apply projects" }),
    );

    await waitFor(() => {
      expect(onResolve).toHaveBeenCalledWith(["worker-demo", "build-lab"]);
    });
  });

  it("autofocuses the filter and restores focus to the trigger on Escape", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });

    try {
      render(
        <OverlayProvider>
          <SelectListOverlayHarness />
        </OverlayProvider>,
      );

      const trigger = screen.getByRole("button", {
        name: "Open project picker",
      });

      trigger.focus();
      fireEvent.click(trigger);

      const dialog = await screen.findByRole("dialog", {
        name: "Select project",
      });
      const filterInput = within(dialog).getByRole("textbox", {
        name: "Filter inventory",
      });

      expect(filterInput).toHaveFocus();

      fireEvent.keyDown(dialog, { key: "Escape" });

      await waitFor(() => {
        expect(
          screen.queryByRole("dialog", { name: "Select project" }),
        ).not.toBeInTheDocument();
        expect(trigger).toHaveFocus();
      });
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("supports keyboard traversal between the filter and result buttons", async () => {
    render(
      <OverlayProvider>
        <SelectListFullScreen
          items={[
            {
              id: "worker-demo",
              label: "Worker Demo",
              subtitle: "Active repository",
            },
            {
              id: "build-lab",
              label: "Build Lab",
              subtitle: "Disabled repository",
            },
            {
              id: "release-forge",
              label: "Release Forge",
              subtitle: "Dormant repository",
            },
          ]}
          title="Select project"
        />
      </OverlayProvider>,
    );

    const filterInput = await screen.findByRole("textbox", {
      name: "Filter inventory",
    });
    const workerDemoButton = screen.getByRole("button", {
      name: /Worker Demo.*Active repository.*Select/i,
    });
    const buildLabButton = screen.getByRole("button", {
      name: /Build Lab.*Disabled repository.*Select/i,
    });
    const releaseForgeButton = screen.getByRole("button", {
      name: /Release Forge.*Dormant repository.*Select/i,
    });

    expect(filterInput).toHaveFocus();

    fireEvent.keyDown(filterInput, { key: "ArrowDown" });
    expect(workerDemoButton).toHaveFocus();

    fireEvent.keyDown(workerDemoButton, { key: "ArrowDown" });
    expect(buildLabButton).toHaveFocus();

    fireEvent.keyDown(buildLabButton, { key: "End" });
    expect(releaseForgeButton).toHaveFocus();

    fireEvent.keyDown(releaseForgeButton, { key: "Home" });
    expect(workerDemoButton).toHaveFocus();

    fireEvent.keyDown(workerDemoButton, { key: "ArrowUp" });
    expect(filterInput).toHaveFocus();
  });

  it("restores focus to the trigger when the picker is closed from its close button", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });

    try {
      render(
        <OverlayProvider>
          <SelectListOverlayHarness />
        </OverlayProvider>,
      );

      const trigger = screen.getByRole("button", {
        name: "Open project picker",
      });

      trigger.focus();
      fireEvent.click(trigger);

      const dialog = await screen.findByRole("dialog", {
        name: "Select project",
      });

      fireEvent.click(
        within(dialog).getByRole("button", { name: "Close overlay" }),
      );

      await waitFor(() => {
        expect(
          screen.queryByRole("dialog", { name: "Select project" }),
        ).not.toBeInTheDocument();
        expect(trigger).toHaveFocus();
      });
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });
});

function SelectListOverlayHarness() {
  const { openOverlay } = useOverlay();

  return (
    <button
      onClick={() => {
        void openOverlay(SelectListFullScreen, {
          initialValues: ["worker-demo"],
          items: [
            {
              id: "worker-demo",
              label: "Worker Demo",
              subtitle: "Active repository",
            },
            {
              id: "build-lab",
              label: "Build Lab",
              subtitle: "Disabled repository",
            },
          ],
          selectionMode: "multiple",
          selectionLabel: "Selected",
          submitLabel: "Apply projects",
          title: "Select project",
        });
      }}
      type="button"
    >
      Open project picker
    </button>
  );
}