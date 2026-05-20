import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ProcessFeedItem } from "./ProcessFeedItem";
import type { ProcessFeedRecord } from "./processFeedPresentation";

afterEach(() => {
  cleanup();
});

describe("ProcessFeedItem", () => {
  it("uses a shared summary strip and opens the detail action", () => {
    const onOpenDetail = vi.fn();

    render(
      <ProcessFeedItem onOpenDetail={onOpenDetail} process={PROCESS_RECORD} />,
    );

    const card = screen
      .getByRole("button", { name: "Open process detail #77" })
      .closest("article");

    expect(card).not.toBeNull();
    expect(screen.getByText("v0.1.0")).toBeInTheDocument();
    expect(screen.getByText("engine: unity")).toBeInTheDocument();
    expect(screen.getByText("1 build")).toBeInTheDocument();
    expect(
      (card as HTMLElement).querySelector(
        ".process-item__summary-strip.ui-summary-strip",
      ),
    ).not.toBeNull();

    fireEvent.click(
      screen.getByRole("button", { name: "Open process detail #77" }),
    );

    expect(onOpenDetail).toHaveBeenCalledWith(PROCESS_RECORD);
  });
});

const PROCESS_RECORD: ProcessFeedRecord = {
  canceled_build_runs: 0,
  canceled_publish_runs: 0,
  created_at: "2026-05-19T00:00:00Z",
  current_step_detail: "Build and publish completed.",
  current_step_label: "Completed",
  current_step_status: "succeeded",
  display_status: "succeeded",
  engine_version: "6000.0.23f1",
  error_message: null,
  failed_build_runs: 0,
  failed_publish_runs: 0,
  finished_at: "2026-05-19T00:12:00Z",
  git_commit: "abc1234",
  git_tag: "v0.1.0",
  queued_build_runs: 0,
  queued_publish_runs: 0,
  release_run_id: 77,
  repository_engine_kind: "unity",
  repository_id: 1,
  repository_name: "Worker Demo",
  repository_url: "https://github.com/indiegabo/worker-demo.git",
  running_build_runs: 0,
  running_publish_runs: 0,
  started_at: "2026-05-19T00:00:10Z",
  succeeded_build_runs: 1,
  succeeded_publish_runs: 1,
  total_build_runs: 1,
  total_publish_runs: 1,
  updated_at: "2026-05-19T00:12:00Z",
};