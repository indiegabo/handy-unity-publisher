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
        automationMode="active"
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
        automationMode="active"
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
        automationMode="active"
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

  it("uses a shared summary strip for runtime and worker totals", () => {
    const { container } = render(
      <WorkerStatusQuickView
        automationMode="active"
        inspectionAvailable
        onResolve={() => undefined}
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

    expect(
      container.querySelector(
        ".worker-status-quick-view__summary-strip.ui-summary-strip",
      ),
    ).not.toBeNull();
  });
});
