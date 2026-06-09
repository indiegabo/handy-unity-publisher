import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const EMPTY_PROCESS_FEED_PAGE = {
  generated_at: "2026-05-19T00:00:00Z",
  has_next_page: false,
  has_previous_page: false,
  items: [],
  page: 1,
  page_size: 5,
  total_items: 0,
  total_pages: 0,
};

const {
  cancelReleaseProcessMock,
  getCurrentWindowMock,
  invokeMock,
  loadRepositoryInspectionMock,
  loadRuntimeAutomationStatusMock,
  loadRuntimeHealthMock,
  loginWithGithubAuthMock,
  reconnectRepositoryAuthMock,
  requestRepositoryInstantCheckMock,
  restartRuntimeMock,
  rerunReleaseProcessMock,
  setRuntimeAutomationModeMock,
  startDraggingMock,
  startRuntimeMock,
  stopRuntimeMock,
  stopRuntimeEventsMock,
  stopRuntimeProcessFeedEventsMock,
  subscribeToProcessFeedEventsMock,
  subscribeToRuntimeEventsMock,
} = vi.hoisted(() => ({
  cancelReleaseProcessMock: vi.fn(),
  getCurrentWindowMock: vi.fn(),
  invokeMock: vi.fn(),
  loadRepositoryInspectionMock: vi.fn(),
  loadRuntimeAutomationStatusMock: vi.fn(),
  loadRuntimeHealthMock: vi.fn(),
  loginWithGithubAuthMock: vi.fn(),
  reconnectRepositoryAuthMock: vi.fn(),
  requestRepositoryInstantCheckMock: vi.fn(),
  restartRuntimeMock: vi.fn(),
  rerunReleaseProcessMock: vi.fn(),
  setRuntimeAutomationModeMock: vi.fn(),
  startDraggingMock: vi.fn(() => Promise.resolve()),
  startRuntimeMock: vi.fn(),
  stopRuntimeMock: vi.fn(),
  stopRuntimeEventsMock: vi.fn(),
  stopRuntimeProcessFeedEventsMock: vi.fn(),
  subscribeToProcessFeedEventsMock: vi.fn(),
  subscribeToRuntimeEventsMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: getCurrentWindowMock,
}));

vi.mock("./services/projects", () => ({
  loadRepositoryInspection: loadRepositoryInspectionMock,
  reconnectRepositoryAuth: reconnectRepositoryAuthMock,
}));

vi.mock("./services/auth", () => ({
  loginWithGithubAuth: loginWithGithubAuthMock,
}));

vi.mock("./services/processFeed", () => ({
  notifyProcessOnHold: vi.fn(),
  subscribeToProcessFeedEvents: subscribeToProcessFeedEventsMock,
}));

vi.mock("./services/runtimeEvents", () => ({
  subscribeToRuntimeEvents: subscribeToRuntimeEventsMock,
}));

vi.mock("./services/processDetail", () => ({
  cancelReleaseProcess: cancelReleaseProcessMock,
  rerunReleaseProcess: rerunReleaseProcessMock,
}));

vi.mock("./services/runtime", () => ({
  loadRuntimeAutomationStatus: loadRuntimeAutomationStatusMock,
  loadRuntimeHealth: loadRuntimeHealthMock,
  requestRepositoryInstantCheck: requestRepositoryInstantCheckMock,
  restartRuntime: restartRuntimeMock,
  setRuntimeAutomationMode: setRuntimeAutomationModeMock,
  startRuntime: startRuntimeMock,
  stopRuntime: stopRuntimeMock,
}));

vi.mock("./components/CreateProjectWizard", () => ({
  CreateProjectWizard: ({
    onCreated,
  }: {
    onCreated: (repositoryId: number) => void;
  }) => (
    <button onClick={() => onCreated(7)} type="button">
      Complete project creation
    </button>
  ),
}));

vi.mock("./components/ProjectsFocusScreen", () => ({
  ProjectsFocusScreen: () => <h1>Project List</h1>,
}));

vi.mock("./components/StartReleaseFocusScreen", () => ({
  StartReleaseFocusScreen: ({
    onQueued,
    repositories,
  }: {
    onQueued: (gitTag: string, repositoryName: string) => void;
    repositories: Array<{ repository_id: number; repository_name: string }>;
  }) => (
    <div>
      <h1>Start release</h1>
      <button onClick={() => onQueued("vT", "Revolutions")} type="button">
        Queue mocked release
      </button>
      {repositories.map((repository) => (
        <div key={repository.repository_id}>{repository.repository_name}</div>
      ))}
    </div>
  ),
}));

import App from "./App";
import OverlayProvider from "./components/OverlayManager";

beforeEach(() => {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });

  getCurrentWindowMock.mockReturnValue({
    startDragging: startDraggingMock,
  });

  invokeMock.mockImplementation(async (command: string) => {
    switch (command) {
      case "main_window_pin_state":
        return false;
      case "process_feed":
        return EMPTY_PROCESS_FEED_PAGE;
      case "transition_window_focus":
      case "close_main_window":
        return undefined;
      case "set_main_window_pinned":
        return true;
      default:
        throw new Error(`Unexpected invoke command: ${command}`);
    }
  });

  loadRuntimeAutomationStatusMock.mockResolvedValue({
    mode: "active",
    updated_at_unix: 1,
  });
  loadRuntimeHealthMock.mockResolvedValue({
    data_dir: "C:/Users/indie/AppData/Roaming/HGP/data",
    database_path: "C:/Users/indie/AppData/Roaming/HGP/data/runtime.sqlite3",
    health_report_path:
      "C:/Users/indie/AppData/Roaming/HGP/data/runtime/health.json",
    log_file_path: "C:/Users/indie/AppData/Roaming/HGP/data/logs/runtime.log",
    log_level: "info",
    platform: "windows-x64",
    process_id: 4242,
    runtime_name: "handy-games-publisher-runtime",
    runtime_version: "0.1.0",
    started_at_unix: 1,
    status: "healthy",
    updated_at_unix: 2,
  });
  subscribeToProcessFeedEventsMock.mockResolvedValue(
    stopRuntimeProcessFeedEventsMock,
  );
  subscribeToRuntimeEventsMock.mockResolvedValue(stopRuntimeEventsMock);
  setRuntimeAutomationModeMock.mockResolvedValue({
    mode: "idle",
    updated_at_unix: 2,
  });
  startRuntimeMock.mockResolvedValue(undefined);
  stopRuntimeMock.mockResolvedValue(undefined);
  restartRuntimeMock.mockResolvedValue(undefined);
  requestRepositoryInstantCheckMock.mockResolvedValue(undefined);
  cancelReleaseProcessMock.mockResolvedValue(undefined);
  rerunReleaseProcessMock.mockResolvedValue(undefined);
  reconnectRepositoryAuthMock.mockResolvedValue(undefined);
  loginWithGithubAuthMock.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("App project creation refresh", () => {
  it("refreshes repository inspection after project creation so Start release sees the new project", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });

    loadRepositoryInspectionMock
      .mockResolvedValueOnce({ repositories: [] })
      .mockResolvedValueOnce({
        repositories: [
          buildRepositoryInspectionEntry({
            local_path: "C:/Users/indie/projetos/Games/Personal/revolutions",
            repo_url: "",
            repository_id: 7,
            repository_name: "Revolutions",
            source_mode: "local_workspace",
          }),
        ],
      });

    try {
      render(
        <OverlayProvider>
          <App />
        </OverlayProvider>,
      );

      fireEvent.click(
        await screen.findByRole("button", { name: "Create project" }),
      );

      fireEvent.click(
        await screen.findByRole("button", {
          name: "Complete project creation",
        }),
      );

      await waitFor(() => {
        expect(loadRepositoryInspectionMock).toHaveBeenCalledTimes(2);
      });

      expect(
        await screen.findByRole("heading", { name: "Project List" }),
      ).toBeInTheDocument();

      fireEvent.click(
        screen.getByRole("button", { name: "Back to main screen" }),
      );

      await waitFor(() => {
        expect(
          screen.getByRole("button", { name: "Start release" }),
        ).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole("button", { name: "Start release" }));

      expect(await screen.findByText("Revolutions")).toBeInTheDocument();
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("clears the queued release banner once the process starts running", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });

    let processFeedCallCount = 0;

    invokeMock.mockImplementation(async (command: string) => {
      switch (command) {
        case "main_window_pin_state":
          return false;
        case "process_feed":
          processFeedCallCount += 1;
          return processFeedCallCount >= 2
            ? {
                ...EMPTY_PROCESS_FEED_PAGE,
                items: [
                  buildProcessFeedRecord({
                    current_step_label: "Building Windows",
                    display_status: "running",
                    git_tag: "vT",
                    repository_name: "Revolutions",
                  }),
                ],
                total_items: 1,
                total_pages: 1,
              }
            : EMPTY_PROCESS_FEED_PAGE;
        case "transition_window_focus":
        case "close_main_window":
          return undefined;
        case "set_main_window_pinned":
          return true;
        default:
          throw new Error(`Unexpected invoke command: ${command}`);
      }
    });

    loadRepositoryInspectionMock.mockResolvedValue({
      repositories: [
        buildRepositoryInspectionEntry({
          local_path: "C:/Users/indie/projetos/Games/Personal/revolutions",
          repo_url: "",
          repository_id: 7,
          repository_name: "Revolutions",
          source_mode: "local_workspace",
        }),
      ],
    });

    try {
      render(
        <OverlayProvider>
          <App />
        </OverlayProvider>,
      );

      fireEvent.click(
        await screen.findByRole("button", { name: "Start release" }),
      );

      fireEvent.click(
        await screen.findByRole("button", { name: "Queue mocked release" }),
      );

      await waitFor(() => {
        expect(
          screen.queryByText("Queued release vT for Revolutions."),
        ).not.toBeInTheDocument();
      });

      expect(await screen.findByText("Building Windows")).toBeInTheDocument();
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });
});

function buildProcessFeedRecord(
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
    created_at: "2026-05-26T17:15:00Z",
    current_step_detail: null,
    current_step_label: "Building Windows",
    current_step_status: "running",
    display_status: "running",
    engine_version: "6000.4.3f1",
    error_message: null,
    failed_build_runs: 0,
    failed_publish_runs: 0,
    finished_at: null,
    git_commit: null,
    git_tag: "vT",
    queued_build_runs: 0,
    queued_publish_runs: 0,
    release_run_id: 1,
    repository_engine_kind: "unity",
    repository_id: 7,
    repository_name: "Revolutions",
    repository_url: null,
    running_build_runs: 1,
    running_publish_runs: 0,
    started_at: "2026-05-26T17:16:00Z",
    succeeded_build_runs: 0,
    succeeded_publish_runs: 0,
    total_build_runs: 2,
    total_publish_runs: 0,
    updated_at: "2026-05-26T17:16:01Z",
    ...overrides,
  };
}

function buildRepositoryInspectionEntry(
  overrides: Partial<{
    artifacts_root_override: string | null;
    auth_binding_status: string;
    auth_last_verified_at: string | null;
    auth_requirement_status: string;
    auth_status_message: string;
    build_targets: Array<Record<string, unknown>>;
    credentials: null;
    default_branch: string | null;
    enabled_build_target_count: number;
    engine_kind: string;
    enabled: boolean;
    last_seen_tag: string | null;
    local_path: string | null;
    pending_release_count: number;
    polling_interval_seconds: number;
    publish_targets: Array<Record<string, unknown>>;
    queued_build_runs: number;
    queued_publish_runs: number;
    release_queue: Array<Record<string, unknown>>;
    repo_url: string;
    repository_id: number;
    repository_name: string;
    running_build_runs: number;
    running_publish_runs: number;
    source_instance_url: string | null;
    source_mode: string | null;
    source_provider_id: string | null;
    visibility_status: string;
    workspace_root_override: string | null;
  }> = {},
) {
  return {
    artifacts_root_override: null,
    auth_binding_status: "not_required",
    auth_last_verified_at: null,
    auth_requirement_status: "not_required",
    auth_status_message: "Local workspace projects do not need auth.",
    build_targets: [
      {
        build_target_id: 11,
        diagnostic_message: "",
        diagnostic_status: "ready",
        enabled: true,
        host_native_diagnostics: null,
        repository_id: overrides.repository_id ?? 1,
        repository_name: overrides.repository_name ?? "Worker Demo",
        runner_type: "host-native",
        target_name: "Windows Build",
        unity_build_method: "Builder.PerformBuild",
        unity_target_platform: "StandaloneWindows64",
      },
    ],
    credentials: null,
    default_branch: null,
    enabled: true,
    enabled_build_target_count: 1,
    engine_kind: "unity",
    last_seen_tag: null,
    local_path: null,
    pending_release_count: 0,
    polling_interval_seconds: 300,
    publish_targets: [],
    queued_build_runs: 0,
    queued_publish_runs: 0,
    release_queue: [],
    repo_url: "",
    repository_id: 1,
    repository_name: "Worker Demo",
    running_build_runs: 0,
    running_publish_runs: 0,
    source_instance_url: null,
    source_mode: "local_workspace",
    source_provider_id: null,
    visibility_status: "local_only",
    workspace_root_override: null,
    ...overrides,
  };
}
