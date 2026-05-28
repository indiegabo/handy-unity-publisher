import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import OverlayProvider from "./OverlayManager";
import { ProcessDetailFocusScreen } from "./ProcessDetailFocusScreen";
import type { ProcessFeedRecord } from "./processFeedPresentation";

const {
  deleteReleaseProcessOutputsMock,
  loadArtifactInspectionMock,
  loadBuildExecutionReportMock,
  loadBuildHistoryMock,
  openHostPathMock,
  purgeBuildExecutionRetentionMock,
  readRetainedLogArchiveEntryMock,
  requestRerunMock,
} = vi.hoisted(() => ({
  deleteReleaseProcessOutputsMock: vi.fn(),
  loadArtifactInspectionMock: vi.fn(),
  loadBuildExecutionReportMock: vi.fn(),
  loadBuildHistoryMock: vi.fn(),
  openHostPathMock: vi.fn(),
  purgeBuildExecutionRetentionMock: vi.fn(),
  readRetainedLogArchiveEntryMock: vi.fn(),
  requestRerunMock: vi.fn(),
}));

vi.mock("../services/processDetail", () => ({
  deleteReleaseProcessOutputs: deleteReleaseProcessOutputsMock,
  loadArtifactInspection: loadArtifactInspectionMock,
  loadBuildExecutionReport: loadBuildExecutionReportMock,
  loadBuildHistory: loadBuildHistoryMock,
  openHostPath: openHostPathMock,
  purgeBuildExecutionRetention: purgeBuildExecutionRetentionMock,
  readRetainedLogArchiveEntry: readRetainedLogArchiveEntryMock,
}));

const COMPLETED_PROCESS: ProcessFeedRecord = {
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

const ON_HOLD_PROCESS: ProcessFeedRecord = {
  ...COMPLETED_PROCESS,
  current_step_detail:
    "Process on hold because Unity Editor appears to be open for the local workspace.",
  current_step_label: "Awaiting Unity editor lock release",
  current_step_status: "on_hold",
  display_status: "on_hold",
  finished_at: null,
  running_build_runs: 1,
  succeeded_build_runs: 0,
  succeeded_publish_runs: 0,
  updated_at: "2026-05-19T00:03:00Z",
};

afterEach(() => {
  cleanup();
  document.body.style.overflow = "";
  vi.clearAllMocks();
});

beforeEach(() => {
  loadBuildHistoryMock.mockResolvedValue([
    {
      artifact_count: 0,
      artifact_root_path: "C:/tmp/artifacts",
      build_run_id: 11,
      build_target_id: 5,
      build_target_name: "Windows Build",
      created_at: "2026-05-19T00:00:20Z",
      engine_version: "6000.0.23f1",
      error_message: null,
      finished_at: "2026-05-19T00:08:00Z",
      git_commit: "abc1234",
      git_tag: "v0.1.0",
      image_ref: null,
      log_path: "C:/tmp/workspace/build.log",
      publish_run_count: 1,
      release_run_id: 77,
      repository_id: 1,
      repository_name: "Worker Demo",
      repository_url: "https://github.com/indiegabo/worker-demo.git",
      runner_type: "host-native",
      started_at: "2026-05-19T00:00:10Z",
      status: "succeeded",
      unity_build_method: "Builder.PerformBuild",
      unity_target_platform: "StandaloneWindows64",
      updated_at: "2026-05-19T00:08:00Z",
      workspace_path: "C:/tmp/workspace",
    },
  ]);

  loadArtifactInspectionMock.mockResolvedValue([]);
  loadBuildExecutionReportMock.mockResolvedValue({
    build_run_id: 11,
    exists: true,
    log_entries: [
      {
        compressed_size_bytes: 320,
        entry_name: "Editor.log",
        entry_path: "logs/Editor.log",
        size_bytes: 1500,
      },
    ],
    logs_archive_exists: true,
    logs_archive_path: "C:/tmp/retained/execution-logs.zip",
    report: {
      status: "ok",
      summary: {
        builds: 1,
        publishes: 1,
      },
    },
    report_path: "C:/tmp/retained/report.json",
    retained_dir_path: "C:/tmp/retained",
    workspace_path: "C:/tmp/workspace",
  });
  readRetainedLogArchiveEntryMock.mockResolvedValue({
    archive_path: "C:/tmp/retained/execution-logs.zip",
    content: "Build completed successfully.",
    entry_path: "logs/Editor.log",
    exists: true,
    size_bytes: 1500,
    truncated: false,
  });
  deleteReleaseProcessOutputsMock.mockResolvedValue({
    artifact_root_path: "C:/tmp/artifacts",
    missing_paths: [],
    release_run_id: 77,
    removed_paths: [],
  });
  purgeBuildExecutionRetentionMock.mockResolvedValue({
    build_run_id: 11,
    removed_paths: [],
    retained_dir_path: "C:/tmp/retained",
    workspace_path: "C:/tmp/workspace",
    workspace_removed: false,
  });
  openHostPathMock.mockResolvedValue(undefined);
  requestRerunMock.mockResolvedValue(undefined);
});

describe("ProcessDetailFocusScreen", () => {
  it("shows a retryable completed-snapshot error state without fake empty outputs", async () => {
    loadBuildHistoryMock.mockRejectedValueOnce(new Error("Snapshot offline"));

    render(
      <OverlayProvider>
        <ProcessDetailFocusScreen
          process={COMPLETED_PROCESS}
          usesLiveSnapshot
        />
      </OverlayProvider>,
    );

    expect(await screen.findByText("Snapshot offline")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Retry retained data" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(
        "No artifact records are currently attached to this process.",
      ),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(
        "No retained log archive was found for this completed process.",
      ),
    ).not.toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "Retry retained data" }),
    );

    expect(
      await screen.findByRole("button", { name: "View JSON report" }),
    ).toBeInTheDocument();
  });

  it("keeps the last completed snapshot visible when a refresh fails", async () => {
    render(
      <OverlayProvider>
        <ProcessDetailFocusScreen
          process={COMPLETED_PROCESS}
          usesLiveSnapshot
        />
      </OverlayProvider>,
    );

    expect(
      await screen.findByRole("button", { name: "View JSON report" }),
    ).toBeInTheDocument();

    loadBuildHistoryMock.mockRejectedValueOnce(new Error("Snapshot offline"));

    fireEvent.click(
      screen.getByRole("button", { name: "Refresh retained data" }),
    );

    expect(await screen.findByText("Snapshot offline")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Showing the last known completed snapshot while retained data refresh recovers.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "View JSON report" }),
    ).toBeInTheDocument();
  });

  it("requires confirmation before deleting process outputs", async () => {
    render(
      <OverlayProvider>
        <ProcessDetailFocusScreen
          process={COMPLETED_PROCESS}
          usesLiveSnapshot
        />
      </OverlayProvider>,
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "Delete outputs" }),
    );

    const dialog = await screen.findByRole("dialog", {
      name: "Delete outputs?",
    });

    expect(dialog).toBeInTheDocument();
    expect(deleteReleaseProcessOutputsMock).not.toHaveBeenCalled();

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Delete outputs" }),
    );

    await waitFor(() => {
      expect(deleteReleaseProcessOutputsMock).toHaveBeenCalledWith(77);
    });
  });

  it("requires confirmation before deleting retained material", async () => {
    render(
      <OverlayProvider>
        <ProcessDetailFocusScreen
          process={COMPLETED_PROCESS}
          usesLiveSnapshot
        />
      </OverlayProvider>,
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "Delete retained material" }),
    );

    const dialog = await screen.findByRole("dialog", {
      name: "Delete retained material?",
    });

    expect(dialog).toBeInTheDocument();
    expect(purgeBuildExecutionRetentionMock).not.toHaveBeenCalled();

    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "Delete retained material",
      }),
    );

    await waitFor(() => {
      expect(purgeBuildExecutionRetentionMock).toHaveBeenCalledWith(11);
    });
  });

  it("requires confirmation before rerunning the process", async () => {
    render(
      <OverlayProvider>
        <ProcessDetailFocusScreen
          onRequestRerun={requestRerunMock}
          process={COMPLETED_PROCESS}
          usesLiveSnapshot
        />
      </OverlayProvider>,
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "Rerun process" }),
    );

    const dialog = await screen.findByRole("dialog", {
      name: "Rerun process?",
    });

    expect(dialog).toBeInTheDocument();
    expect(requestRerunMock).not.toHaveBeenCalled();

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Rerun process" }),
    );

    await waitFor(() => {
      expect(requestRerunMock).toHaveBeenCalledWith(COMPLETED_PROCESS);
    });
  });

  it("renders on-hold guidance and requires confirmation before canceling", async () => {
    const requestCancelMock = vi.fn().mockResolvedValue(undefined);

    render(
      <OverlayProvider>
        <ProcessDetailFocusScreen
          onRequestCancel={requestCancelMock}
          process={ON_HOLD_PROCESS}
          usesLiveSnapshot
        />
      </OverlayProvider>,
    );

    expect(
      await screen.findByText(
        "Close Unity Editor to continue this process. HGP blocks this step intentionally to keep automation consistent, because changing files while a local snapshot is being prepared can invalidate build inputs.",
      ),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Cancel process" }));

    const dialog = await screen.findByRole("dialog", {
      name: "Cancel process?",
    });

    expect(dialog).toBeInTheDocument();
    expect(requestCancelMock).not.toHaveBeenCalled();
    expect(
      within(dialog).getByText(
        "Use this when you do not want to close Unity right now. To continue this run, close Unity Editor and HGP will resume from the on-hold gate.",
      ),
    ).toBeInTheDocument();

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Cancel process" }),
    );

    await waitFor(() => {
      expect(requestCancelMock).toHaveBeenCalledWith(ON_HOLD_PROCESS);
    });

    expect(
      await screen.findByText(
        "Cancel request accepted. The process feed will refresh as soon as the runtime snapshot advances.",
      ),
    ).toBeInTheDocument();
  });

  it("uses shared panel summaries for the outcome and runtime metadata panels", async () => {
    render(
      <OverlayProvider>
        <ProcessDetailFocusScreen
          process={COMPLETED_PROCESS}
          usesLiveSnapshot
        />
      </OverlayProvider>,
    );

    await screen.findByRole("button", { name: "View JSON report" });

    const finalOutcomePanel = screen
      .getByRole("heading", { name: "Final Outcome" })
      .closest("section");
    const runtimeMetadataPanel = screen
      .getByRole("heading", { name: "Runtime Metadata" })
      .closest("section");

    expect(finalOutcomePanel).not.toBeNull();
    expect(runtimeMetadataPanel).not.toBeNull();
    expect(
      (finalOutcomePanel as HTMLElement).querySelector(".ui-panel__summary"),
    ).not.toBeNull();
    expect(
      (runtimeMetadataPanel as HTMLElement).querySelector(".ui-panel__summary"),
    ).not.toBeNull();
  });

  it("shows local workspace when the release has no repository URL", async () => {
    render(
      <OverlayProvider>
        <ProcessDetailFocusScreen
          process={{
            ...COMPLETED_PROCESS,
            repository_url: null,
          }}
          usesLiveSnapshot
        />
      </OverlayProvider>,
    );

    expect(await screen.findByText("Local workspace")).toBeInTheDocument();
  });

  it("opens retained report JSON in the log viewer overlay", async () => {
    render(
      <OverlayProvider>
        <ProcessDetailFocusScreen
          process={COMPLETED_PROCESS}
          usesLiveSnapshot
        />
      </OverlayProvider>,
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "View JSON report" }),
    );

    const dialog = await screen.findByRole("dialog", {
      name: "Retained report JSON",
    });

    expect(within(dialog).getByText(/"builds": 1/i)).toBeInTheDocument();
  });

  it("dismisses the retained report viewer with Escape and restores focus to its trigger", async () => {
    render(
      <OverlayProvider>
        <ProcessDetailFocusScreen
          process={COMPLETED_PROCESS}
          usesLiveSnapshot
        />
      </OverlayProvider>,
    );

    const reportViewerButton = await screen.findByRole("button", {
      name: "View JSON report",
    });

    reportViewerButton.focus();
    fireEvent.click(reportViewerButton);

    const dialog = await screen.findByRole("dialog", {
      name: "Retained report JSON",
    });

    fireEvent.keyDown(dialog, { key: "Escape" });

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "Retained report JSON" }),
      ).not.toBeInTheDocument();
      expect(reportViewerButton).toHaveFocus();
    });
  });

  it("loads a retained log entry and opens it in the log viewer overlay", async () => {
    render(
      <OverlayProvider>
        <ProcessDetailFocusScreen
          process={COMPLETED_PROCESS}
          usesLiveSnapshot
        />
      </OverlayProvider>,
    );

    fireEvent.click(
      await screen.findByRole("button", {
        name: "Expand retained log Editor.log",
      }),
    );

    fireEvent.click(
      await screen.findByRole("button", {
        name: "Open retained log viewer for Editor.log",
      }),
    );

    await waitFor(() => {
      expect(readRetainedLogArchiveEntryMock).toHaveBeenCalledWith(
        11,
        "logs/Editor.log",
        128 * 1024,
      );
    });

    const dialog = await screen.findByRole("dialog", { name: "Editor.log" });

    expect(
      within(dialog).getByText("Build completed successfully."),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByText(/Showing the full log file/i),
    ).toBeInTheDocument();
  });

  it("dismisses the retained log viewer from its close button and restores focus to the trigger", async () => {
    render(
      <OverlayProvider>
        <ProcessDetailFocusScreen
          process={COMPLETED_PROCESS}
          usesLiveSnapshot
        />
      </OverlayProvider>,
    );

    fireEvent.click(
      await screen.findByRole("button", {
        name: "Expand retained log Editor.log",
      }),
    );

    const openViewerButton = await screen.findByRole("button", {
      name: "Open retained log viewer for Editor.log",
    });

    openViewerButton.focus();
    fireEvent.click(openViewerButton);

    const dialog = await screen.findByRole("dialog", { name: "Editor.log" });

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Close overlay" }),
    );

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "Editor.log" }),
      ).not.toBeInTheDocument();
      expect(openViewerButton).toHaveFocus();
    });
  });

  it("opens an artifact viewer overlay and forwards host actions from it", async () => {
    loadArtifactInspectionMock.mockResolvedValueOnce([
      {
        artifact_active_location_kind: "filesystem_absolute",
        artifact_active_location_ref: "C:/tmp/artifacts/build.zip",
        artifact_id: 101,
        artifact_kind: "build-archive",
        artifact_name: "build.zip",
        artifact_path: "build.zip",
        artifact_root_path: "C:/tmp/artifacts",
        build_run_id: 11,
        build_status: "succeeded",
        build_target_id: 5,
        build_target_name: "Windows Build",
        canceled_publish_runs: 0,
        checksum_sha256: null,
        created_at: "2026-05-19T00:08:10Z",
        failed_publish_runs: 0,
        git_commit: "abc1234",
        git_tag: "v0.1.0",
        publish_run_count: 1,
        publish_runs: [
          {
            created_at: "2026-05-19T00:09:00Z",
            destination_ref: "D:/releases/build.zip",
            publish_run_id: 301,
            publish_target_id: 21,
            publish_target_kind: "filesystem",
            publish_target_name: "Production Share",
            status: "succeeded",
            updated_at: "2026-05-19T00:09:10Z",
          },
        ],
        queued_publish_runs: 0,
        release_run_id: 77,
        repository_id: 1,
        repository_name: "Worker Demo",
        repository_url: "https://github.com/indiegabo/worker-demo.git",
        running_publish_runs: 0,
        size_bytes: 2048,
        succeeded_publish_runs: 1,
        unity_target_platform: "StandaloneWindows64",
      },
    ]);

    render(
      <OverlayProvider>
        <ProcessDetailFocusScreen
          process={COMPLETED_PROCESS}
          usesLiveSnapshot
        />
      </OverlayProvider>,
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "Inspect artifact" }),
    );

    const dialog = await screen.findByRole("dialog", { name: "build.zip" });

    expect(
      within(dialog).getByText(/In-shell preview is not available/i),
    ).toBeInTheDocument();

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Open artifact" }),
    );

    await waitFor(() => {
      expect(openHostPathMock).toHaveBeenCalledWith(
        "C:/tmp/artifacts/build.zip",
      );
    });
  });

  it("dismisses the artifact viewer from its close button and restores focus to the trigger", async () => {
    loadArtifactInspectionMock.mockResolvedValueOnce([
      {
        artifact_active_location_kind: "filesystem_absolute",
        artifact_active_location_ref: "C:/tmp/artifacts/build.zip",
        artifact_id: 101,
        artifact_kind: "build-archive",
        artifact_name: "build.zip",
        artifact_path: "build.zip",
        artifact_root_path: "C:/tmp/artifacts",
        build_run_id: 11,
        build_status: "succeeded",
        build_target_id: 5,
        build_target_name: "Windows Build",
        canceled_publish_runs: 0,
        checksum_sha256: null,
        created_at: "2026-05-19T00:08:10Z",
        failed_publish_runs: 0,
        git_commit: "abc1234",
        git_tag: "v0.1.0",
        publish_run_count: 1,
        publish_runs: [],
        queued_publish_runs: 0,
        release_run_id: 77,
        repository_id: 1,
        repository_name: "Worker Demo",
        repository_url: "https://github.com/indiegabo/worker-demo.git",
        running_publish_runs: 0,
        size_bytes: 2048,
        succeeded_publish_runs: 0,
        unity_target_platform: "StandaloneWindows64",
      },
    ]);

    render(
      <OverlayProvider>
        <ProcessDetailFocusScreen
          process={COMPLETED_PROCESS}
          usesLiveSnapshot
        />
      </OverlayProvider>,
    );

    const inspectArtifactButton = await screen.findByRole("button", {
      name: "Inspect artifact",
    });

    inspectArtifactButton.focus();
    fireEvent.click(inspectArtifactButton);

    const dialog = await screen.findByRole("dialog", { name: "build.zip" });

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Close overlay" }),
    );

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "build.zip" }),
      ).not.toBeInTheDocument();
      expect(inspectArtifactButton).toHaveFocus();
    });
  });
});
