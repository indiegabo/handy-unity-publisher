import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const runtimeElapsedMocks = vi.hoisted(() => {
  const listeners = new Map<number, (elapsedClock: string) => void>();

  return {
    clearListeners() {
      listeners.clear();
    },
    emitElapsedClock(releaseRunId: number, elapsedClock: string) {
      listeners.get(releaseRunId)?.(elapsedClock);
    },
    subscribeToProcessElapsedClockMock: vi.fn(
      async (
        releaseRunId: number,
        listener: (elapsedClock: string) => void,
      ) => {
        listeners.set(releaseRunId, listener);

        return () => {
          listeners.delete(releaseRunId);
        };
      },
    ),
  };
});

vi.mock("../services/runtimeEvents", () => ({
  subscribeToProcessElapsedClock:
    runtimeElapsedMocks.subscribeToProcessElapsedClockMock,
}));

import { ProcessFeedItem } from "./ProcessFeedItem";
import type { ProcessFeedRecord } from "./processFeedPresentation";

afterEach(() => {
  cleanup();
  runtimeElapsedMocks.clearListeners();
  vi.clearAllMocks();
  vi.useRealTimers();
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
    expect(screen.queryByText("Elapsed Time")).not.toBeInTheDocument();
    expect(screen.getByText("00:12:00")).toBeInTheDocument();
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

  it("shows on-hold reason and requests interruption directly from the card", () => {
    const onOpenDetail = vi.fn();
    const onRequestCancel = vi.fn();

    render(
      <ProcessFeedItem
        onOpenDetail={onOpenDetail}
        onRequestCancel={onRequestCancel}
        process={{
          ...PROCESS_RECORD,
          current_step_detail:
            "Process on hold because Unity Editor appears to be open for local workspace.",
          current_step_status: "on_hold",
          display_status: "on_hold",
          finished_at: null,
          running_build_runs: 1,
        }}
      />,
    );

    expect(
      screen.getByText(
        "Process on hold because Unity Editor appears to be open for local workspace.",
      ),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Interrupt process" }));

    expect(onRequestCancel).toHaveBeenCalledTimes(1);
    expect(onOpenDetail).not.toHaveBeenCalled();
  });

  it("shows a live hh:mm:ss clock for the whole process and the current stage detail for active work", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-05-19T00:00:15Z"));

    render(
      <ProcessFeedItem
        onOpenDetail={vi.fn()}
        process={{
          ...PROCESS_RECORD,
          current_step_detail:
            "Receiving objects: 42% (42/100), 1.20 MiB | 256.00 KiB/s",
          current_step_label: "Checking out source",
          current_step_status: "running",
          display_status: "running",
          finished_at: null,
        }}
      />,
    );

    expect(
      runtimeElapsedMocks.subscribeToProcessElapsedClockMock,
    ).toHaveBeenCalledWith(77, expect.any(Function));
    expect(screen.getByText("00:00:15")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Receiving objects: 42% (42/100), 1.20 MiB | 256.00 KiB/s",
      ),
    ).toBeInTheDocument();

    act(() => {
      runtimeElapsedMocks.emitElapsedClock(91, "00:00:44");
      runtimeElapsedMocks.emitElapsedClock(77, "00:00:17");
    });

    expect(screen.getByText("00:00:17")).toBeInTheDocument();
    expect(screen.queryByText("00:00:44")).not.toBeInTheDocument();
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
