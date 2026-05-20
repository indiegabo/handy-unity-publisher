import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WorkerStatusQuickView } from "./WorkerStatusQuickView";

afterEach(() => {
  cleanup();
});

describe("WorkerStatusQuickView", () => {
  it("focuses the loading copy when inspection is still unavailable", async () => {
    render(
      <WorkerStatusQuickView
        inspectionAvailable={false}
        onResolve={() => undefined}
        projectWorkers={[]}
        runtimeStatus={null}
      />,
    );

    await waitFor(() => {
      expect(
        screen.getByText("Loading project worker inventory..."),
      ).toHaveFocus();
    });
  });

  it("focuses the empty-state copy when no project workers are configured", async () => {
    render(
      <WorkerStatusQuickView
        inspectionAvailable
        onResolve={() => undefined}
        projectWorkers={[]}
        runtimeStatus="healthy"
      />,
    );

    await waitFor(() => {
      expect(
        screen.getByText("No active project workers configured."),
      ).toHaveFocus();
    });
  });

  it("focuses the primary action when worker inventory is available", async () => {
    const onResolve = vi.fn();

    render(
      <WorkerStatusQuickView
        inspectionAvailable
        onResolve={onResolve}
        projectWorkers={[
          {
            buildTargets: [
              {
                buildTargetId: 7,
                diagnosticMessage: "Ready",
                diagnosticStatus: "ready",
                name: "Windows",
                unityTargetPlatform: "StandaloneWindows64",
              },
            ],
            pollingIntervalSeconds: 30,
            repositoryId: 1,
            repositoryName: "Worker Demo",
          },
        ]}
        runtimeStatus="healthy"
      />,
    );

    const action = screen.getByRole("button", { name: "Open Project Workers" });

    await waitFor(() => {
      expect(action).toHaveFocus();
    });
  });
});
