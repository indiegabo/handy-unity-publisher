import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ProcessHistoryFocusScreen } from "./ProcessHistoryFocusScreen";

const { loadProcessFeedMock, subscribeToProcessFeedEventsMock } = vi.hoisted(
  () => ({
    loadProcessFeedMock: vi.fn(),
    subscribeToProcessFeedEventsMock: vi.fn(),
  }),
);

vi.mock("../services/processFeed", async () => {
  const actual = await vi.importActual<typeof import("../services/processFeed")>(
    "../services/processFeed",
  );

  return {
    ...actual,
    loadProcessFeed: loadProcessFeedMock,
    subscribeToProcessFeedEvents: subscribeToProcessFeedEventsMock,
  };
});

const PAGE_ONE = {
  generated_at: "2026-05-26T18:00:00Z",
  has_next_page: true,
  has_previous_page: false,
  items: [buildProcessRecord({ release_run_id: 101 })],
  page: 1,
  page_size: 12,
  total_items: 2,
  total_pages: 2,
};

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

beforeEach(() => {
  subscribeToProcessFeedEventsMock.mockResolvedValue(vi.fn());
});

describe("ProcessHistoryFocusScreen", () => {
  it("loads the archived feed and paginates across the full process history", async () => {
    const onOpenDetail = vi.fn();

    loadProcessFeedMock
      .mockResolvedValueOnce(PAGE_ONE)
      .mockResolvedValueOnce({
        ...PAGE_ONE,
        has_next_page: false,
        has_previous_page: true,
        items: [
          buildProcessRecord({
            current_step_label: "Completed",
            current_step_status: "succeeded",
            display_status: "succeeded",
            release_run_id: 77,
          }),
        ],
        page: 2,
      });

    render(<ProcessHistoryFocusScreen onOpenDetail={onOpenDetail} />);

    expect(
      await screen.findByRole("heading", { name: "Process History" }),
    ).toBeInTheDocument();
    expect(await screen.findByText("Revolutions")).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "Open process detail #101" }),
    );

    expect(onOpenDetail).toHaveBeenCalledWith(
      expect.objectContaining({ release_run_id: 101 }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Next" }));

    await waitFor(() => {
      expect(loadProcessFeedMock).toHaveBeenLastCalledWith({
        page: 2,
        pageSize: 12,
        query: "",
        scope: "all",
        status: "all",
      });
    });

    expect(
      await screen.findByRole("button", { name: "Open process detail #77" }),
    ).toBeInTheDocument();
  });

  it("resets pagination when archive filters change", async () => {
    loadProcessFeedMock
      .mockResolvedValueOnce(PAGE_ONE)
      .mockResolvedValueOnce({
        generated_at: "2026-05-26T18:05:00Z",
        has_next_page: false,
        has_previous_page: false,
        items: [],
        page: 1,
        page_size: 12,
        total_items: 0,
        total_pages: 0,
      })
      .mockResolvedValue({
        generated_at: "2026-05-26T18:05:01Z",
        has_next_page: false,
        has_previous_page: false,
        items: [],
        page: 1,
        page_size: 12,
        total_items: 0,
        total_pages: 0,
      });

    render(<ProcessHistoryFocusScreen onOpenDetail={vi.fn()} />);

    await screen.findByText("Revolutions");

    fireEvent.change(screen.getByRole("textbox", { name: "Filter archive" }), {
      target: { value: "blocked" },
    });

    await waitFor(() => {
      expect(loadProcessFeedMock).toHaveBeenLastCalledWith({
        page: 1,
        pageSize: 12,
        query: "blocked",
        scope: "all",
        status: "all",
      });
    });

    fireEvent.change(screen.getByRole("combobox", { name: "Status" }), {
      target: { value: "failed" },
    });

    await waitFor(() => {
      expect(loadProcessFeedMock).toHaveBeenLastCalledWith({
        page: 1,
        pageSize: 12,
        query: "blocked",
        scope: "all",
        status: "failed",
      });
    });

    expect(
      await screen.findByText("No processes match this filter."),
    ).toBeInTheDocument();
  });
});

function buildProcessRecord(
  overrides: Partial<{
    canceled_build_runs: number;
    canceled_publish_runs: number;
    created_at: string;
    current_step_detail: string | null;
    current_step_label: string;
    current_step_status: string;
    display_status: string;
    engine_version: string | null;
    error_message: string | null;
    failed_build_runs: number;
    failed_publish_runs: number;
    finished_at: string | null;
    git_commit: string | null;
    git_tag: string;
    queued_build_runs: number;
    queued_publish_runs: number;
    release_run_id: number;
    repository_engine_kind: string;
    repository_id: number;
    repository_name: string;
    repository_url: string | null;
    running_build_runs: number;
    running_publish_runs: number;
    started_at: string | null;
    succeeded_build_runs: number;
    succeeded_publish_runs: number;
    total_build_runs: number;
    total_publish_runs: number;
    updated_at: string;
  }> = {},
) {
  return {
    canceled_build_runs: 0,
    canceled_publish_runs: 0,
    created_at: "2026-05-26T18:00:00Z",
    current_step_detail: "Packaging Windows player",
    current_step_label: "Building Windows",
    current_step_status: "running",
    display_status: "running",
    engine_version: "6000.0.23f1",
    error_message: null,
    failed_build_runs: 0,
    failed_publish_runs: 0,
    finished_at: null,
    git_commit: "deadbeef",
    git_tag: "v1.2.0",
    queued_build_runs: 0,
    queued_publish_runs: 0,
    release_run_id: 101,
    repository_engine_kind: "unity",
    repository_id: 7,
    repository_name: "Revolutions",
    repository_url: "https://example.com/revolutions.git",
    running_build_runs: 1,
    running_publish_runs: 0,
    started_at: "2026-05-26T18:01:00Z",
    succeeded_build_runs: 0,
    succeeded_publish_runs: 0,
    total_build_runs: 1,
    total_publish_runs: 0,
    updated_at: "2026-05-26T18:02:00Z",
    ...overrides,
  };
}