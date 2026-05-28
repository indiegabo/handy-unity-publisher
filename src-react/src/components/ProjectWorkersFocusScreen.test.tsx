import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ProjectWorkersFocusScreen } from "./ProjectWorkersFocusScreen";

afterEach(() => {
  cleanup();
});

describe("ProjectWorkersFocusScreen", () => {
  it("shows a retryable error state when worker inspection is unavailable", () => {
    const onRetryInventory = vi.fn();

    render(
      <ProjectWorkersFocusScreen
        actionError={null}
        actionMessage={null}
        automationMode="active"
        inspectionAvailable={false}
        inspectionError="Inspection offline"
        inspectionStale={false}
        onBulkInstantCheck={() => undefined}
        onInstantCheck={() => undefined}
        onRestartRuntime={() => undefined}
        onRetryInventory={onRetryInventory}
        onStartRuntime={() => undefined}
        onStopRuntime={() => undefined}
        pendingBulkInstantCheck={false}
        pendingInstantCheckRepositoryId={null}
        pendingRuntimeAction={null}
        projectWorkers={[]}
        runtimeStatus="healthy"
      />,
    );

    expect(
      screen.getByText("Project worker inventory is unavailable."),
    ).toBeInTheDocument();
    expect(screen.getByText("Inspection offline")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry inventory" }));

    expect(onRetryInventory).toHaveBeenCalledTimes(1);
  });

  it("keeps runtime-wide controls in a dedicated panel above the worker inventory", () => {
    render(
      <ProjectWorkersFocusScreen
        actionError={null}
        actionMessage={null}
        automationMode="active"
        inspectionAvailable
        inspectionError={null}
        inspectionStale={false}
        onBulkInstantCheck={() => undefined}
        onInstantCheck={() => undefined}
        onRestartRuntime={() => undefined}
        onRetryInventory={() => undefined}
        onStartRuntime={() => undefined}
        onStopRuntime={() => undefined}
        pendingBulkInstantCheck={false}
        pendingInstantCheckRepositoryId={null}
        pendingRuntimeAction={null}
        projectWorkers={[]}
        runtimeStatus="healthy"
      />,
    );

    const runtimePanel = screen
      .getByRole("heading", {
        name: "Runtime Controls",
      })
      .closest(".project-workers-runtime-panel");
    const inventoryPanel = screen
      .getByRole("button", {
        name: "Collapse Worker Inventory",
      })
      .closest(".project-workers-section-accordion");

    expect(runtimePanel).not.toBeNull();
    expect(inventoryPanel).not.toBeNull();
    if (!runtimePanel || !inventoryPanel) {
      throw new Error("Expected runtime and inventory panels to be rendered.");
    }

    const runtimePanelElement = runtimePanel as HTMLElement;
    const inventoryPanelElement = inventoryPanel as HTMLElement;

    expect(
      within(runtimePanelElement).getByRole("button", { name: "Start" }),
    ).toBeInTheDocument();
    expect(
      within(runtimePanelElement).getByRole("button", { name: "Stop" }),
    ).toBeInTheDocument();
    expect(
      within(runtimePanelElement).getByRole("button", { name: "Restart" }),
    ).toBeInTheDocument();
    expect(
      within(runtimePanelElement).queryByRole("button", {
        name: /Collapse Worker Inventory|Expand Worker Inventory/,
      }),
    ).not.toBeInTheDocument();
    expect(
      runtimePanelElement.compareDocumentPosition(inventoryPanelElement) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });

  it("toggles a worker accordion body from the chevron button", () => {
    const { container } = render(
      <ProjectWorkersFocusScreen
        actionError={null}
        actionMessage={null}
        automationMode="active"
        inspectionAvailable
        inspectionError={null}
        inspectionStale={false}
        onBulkInstantCheck={() => undefined}
        onInstantCheck={() => undefined}
        onRestartRuntime={() => undefined}
        onRetryInventory={() => undefined}
        onStartRuntime={() => undefined}
        onStopRuntime={() => undefined}
        pendingBulkInstantCheck={false}
        pendingInstantCheckRepositoryId={null}
        pendingRuntimeAction={null}
        projectWorkers={[
          {
            pollingIntervalSeconds: 30,
            repositoryId: 1,
            repositoryName: "Revolutions",
            sourceMode: "managed_repository",
            buildTargets: [
              {
                buildTargetId: 1,
                diagnosticMessage: "Ready",
                diagnosticStatus: "ready",
                name: "Windows",
                unityTargetPlatform: "StandaloneWindows64",
              },
            ],
          },
        ]}
        runtimeStatus="healthy"
      />,
    );

    const accordionBody = container.querySelector(
      ".project-workers-worker-accordion .vertical-accordion__body",
    );

    expect(accordionBody).not.toBeNull();
    expect(accordionBody).toHaveAttribute("aria-hidden", "true");
    expect(accordionBody).toHaveAttribute("hidden");

    fireEvent.click(screen.getByRole("button", { name: "Expand Revolutions" }));

    expect(accordionBody).toHaveAttribute("aria-hidden", "false");
    expect(accordionBody).not.toHaveAttribute("hidden");

    fireEvent.click(
      screen.getByRole("button", { name: "Collapse Revolutions" }),
    );

    expect(accordionBody).toHaveAttribute("aria-hidden", "true");
    expect(accordionBody).toHaveAttribute("hidden");
  });

  it("uses shared summary strips for inventory and worker accordions", () => {
    render(
      <ProjectWorkersFocusScreen
        actionError={null}
        actionMessage={null}
        automationMode="active"
        inspectionAvailable
        inspectionError={null}
        inspectionStale={false}
        onBulkInstantCheck={() => undefined}
        onInstantCheck={() => undefined}
        onRestartRuntime={() => undefined}
        onRetryInventory={() => undefined}
        onStartRuntime={() => undefined}
        onStopRuntime={() => undefined}
        pendingBulkInstantCheck={false}
        pendingInstantCheckRepositoryId={null}
        pendingRuntimeAction={null}
        projectWorkers={[
          {
            pollingIntervalSeconds: 30,
            repositoryId: 1,
            repositoryName: "Revolutions",
            sourceMode: "managed_repository",
            buildTargets: [
              {
                buildTargetId: 1,
                diagnosticMessage: "Ready",
                diagnosticStatus: "ready",
                name: "Windows",
                unityTargetPlatform: "StandaloneWindows64",
              },
            ],
          },
        ]}
        runtimeStatus="healthy"
      />,
    );

    const inventoryAccordion = screen
      .getByRole("button", { name: "Collapse Worker Inventory" })
      .closest(".project-workers-section-accordion");
    const workerAccordion = screen
      .getByRole("button", { name: "Expand Revolutions" })
      .closest(".project-workers-worker-accordion");

    expect(inventoryAccordion).not.toBeNull();
    expect(workerAccordion).not.toBeNull();
    expect(
      (inventoryAccordion as HTMLElement).querySelector(
        ".project-workers-section-accordion__summary.ui-summary-strip",
      ),
    ).not.toBeNull();
    expect(
      (workerAccordion as HTMLElement).querySelector(
        ".project-workers-worker-accordion__summary.ui-summary-strip",
      ),
    ).not.toBeNull();
  });

  it("shows local workspace workers with no remote polling cadence", () => {
    render(
      <ProjectWorkersFocusScreen
        actionError={null}
        actionMessage={null}
        automationMode="active"
        inspectionAvailable
        inspectionError={null}
        inspectionStale={false}
        onBulkInstantCheck={() => undefined}
        onInstantCheck={() => undefined}
        onRestartRuntime={() => undefined}
        onRetryInventory={() => undefined}
        onStartRuntime={() => undefined}
        onStopRuntime={() => undefined}
        pendingBulkInstantCheck={false}
        pendingInstantCheckRepositoryId={null}
        pendingRuntimeAction={null}
        projectWorkers={[
          {
            pollingIntervalSeconds: 0,
            repositoryId: 1,
            repositoryName: "Revolutions",
            sourceMode: "local_workspace",
            buildTargets: [
              {
                buildTargetId: 1,
                diagnosticMessage: "Ready",
                diagnosticStatus: "ready",
                name: "Windows",
                unityTargetPlatform: "StandaloneWindows64",
              },
            ],
          },
        ]}
        runtimeStatus="healthy"
      />,
    );

    expect(screen.getByText("No remote polling")).toBeInTheDocument();
  });
});
