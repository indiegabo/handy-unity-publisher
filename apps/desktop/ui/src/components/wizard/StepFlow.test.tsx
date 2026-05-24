import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { StepFlow } from "./StepFlow";

afterEach(() => {
  cleanup();
});

describe("StepFlow", () => {
  it("renders the active step and allows navigation only to selectable steps", () => {
    const onStepSelect = vi.fn();

    render(
      <StepFlow
        activeStepKey="access"
        endActions={<button type="button">Next</button>}
        onStepSelect={onStepSelect}
        progressSummary={<p>Progress summary</p>}
        startActions={<button type="button">Previous</button>}
        stepSummary={<p>Step summary</p>}
        steps={[
          {
            description: "Identity step",
            key: "identity",
            label: "Identity",
          },
          {
            description: "Access step",
            key: "access",
            label: "Repository",
          },
          {
            description: "Review step",
            key: "review",
            label: "Review",
          },
        ]}
      >
        <p>Current step body</p>
      </StepFlow>,
    );

    expect(
      screen.getByRole("heading", { name: "Repository" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Access step")).toBeInTheDocument();
    expect(screen.getByText("Current step body")).toBeInTheDocument();
    expect(screen.getByText("Progress summary")).toBeInTheDocument();
    expect(screen.getByText("Step summary")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Previous" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Next" })).toBeInTheDocument();

    const completedStep = screen.getByRole("button", { name: /Identity/i });
    const futureStep = screen.getByRole("button", { name: /Review/i });

    expect(completedStep).toBeEnabled();
    expect(futureStep).toBeDisabled();

    fireEvent.click(completedStep);

    expect(onStepSelect).toHaveBeenCalledWith("identity");
  });
});
