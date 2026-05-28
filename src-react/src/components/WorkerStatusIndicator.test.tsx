import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { WorkerStatusIndicator } from "./WorkerStatusIndicator";

describe("WorkerStatusIndicator", () => {
  it("stays a dumb trigger and forwards shell-level trigger semantics", () => {
    const handleClick = vi.fn();

    render(
      <WorkerStatusIndicator
        animated
        aria-expanded="true"
        aria-haspopup="dialog"
        label="Project workers active"
        onClick={handleClick}
        title="Active workers: Worker Demo (Windows Build)"
        tone="warning"
      />,
    );

    const trigger = screen.getByRole("button", {
      name: "Project workers active",
    });

    expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(trigger).toHaveAttribute("aria-haspopup", "dialog");
    expect(trigger).toHaveAttribute(
      "title",
      "Active workers: Worker Demo (Windows Build)",
    );
    expect(trigger).toHaveClass("worker-status-indicator--warning");
    expect(trigger).toHaveClass("worker-status-indicator--animated");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
