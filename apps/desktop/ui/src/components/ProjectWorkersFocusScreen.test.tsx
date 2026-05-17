import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ProjectWorkersFocusScreen } from "./ProjectWorkersFocusScreen";

describe("ProjectWorkersFocusScreen", () => {
  it("toggles a worker accordion body from the chevron button", () => {
    const { container } = render(
      <ProjectWorkersFocusScreen
        actionError={null}
        actionMessage={null}
        inspectionAvailable
        onInstantCheck={() => undefined}
        onRestartRuntime={() => undefined}
        onStartRuntime={() => undefined}
        onStopRuntime={() => undefined}
        pendingInstantCheckRepositoryId={null}
        pendingRuntimeAction={null}
        projectWorkers={[
          {
            pollingIntervalSeconds: 30,
            repositoryId: 1,
            repositoryName: "Revolutions",
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

    fireEvent.click(screen.getByRole("button", { name: "Collapse Revolutions" }));

    expect(accordionBody).toHaveAttribute("aria-hidden", "true");
    expect(accordionBody).toHaveAttribute("hidden");
  });
});