import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const {
  createRepositoryProjectMock,
  detectRepositoryProviderMock,
  getCurrentWindowMock,
  invokeMock,
  loadAuthProvidersMock,
  loadRepositoryInspectionMock,
  loadSecretSettingsMock,
  loginWithGithubAuthMock,
  loadRuntimeHealthMock,
  reconnectRepositoryAuthMock,
  rerunReleaseProcessMock,
  requestRepositoryInstantCheckMock,
  restartRuntimeMock,
  saveSecretCredentialMock,
  startDraggingMock,
  startRuntimeMock,
  stopRuntimeMock,
  stopRuntimeProcessFeedEventsMock,
  subscribeToProcessFeedEventsMock,
  stopRuntimeEventsMock,
  subscribeToRuntimeEventsMock,
  validateUnityExecutablePathMock,
} = vi.hoisted(() => ({
  createRepositoryProjectMock: vi.fn(),
  detectRepositoryProviderMock: vi.fn(),
  getCurrentWindowMock: vi.fn(),
  invokeMock: vi.fn(),
  loadAuthProvidersMock: vi.fn(),
  loadRepositoryInspectionMock: vi.fn(),
  loadSecretSettingsMock: vi.fn(),
  loginWithGithubAuthMock: vi.fn(),
  loadRuntimeHealthMock: vi.fn(),
  reconnectRepositoryAuthMock: vi.fn(),
  rerunReleaseProcessMock: vi.fn(),
  requestRepositoryInstantCheckMock: vi.fn(),
  restartRuntimeMock: vi.fn(),
  saveSecretCredentialMock: vi.fn(),
  startDraggingMock: vi.fn(() => Promise.resolve()),
  startRuntimeMock: vi.fn(),
  stopRuntimeMock: vi.fn(),
  stopRuntimeProcessFeedEventsMock: vi.fn(),
  subscribeToProcessFeedEventsMock: vi.fn(),
  stopRuntimeEventsMock: vi.fn(),
  subscribeToRuntimeEventsMock: vi.fn(),
  validateUnityExecutablePathMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: getCurrentWindowMock,
}));

vi.mock("./services/projects", () => ({
  createRepositoryProject: createRepositoryProjectMock,
  detectRepositoryProvider: detectRepositoryProviderMock,
  loadRepositoryInspection: loadRepositoryInspectionMock,
  loadSecretSettings: loadSecretSettingsMock,
  reconnectRepositoryAuth: reconnectRepositoryAuthMock,
  saveSecretCredential: saveSecretCredentialMock,
  validateUnityExecutablePath: validateUnityExecutablePathMock,
}));

vi.mock("./services/auth", () => ({
  loadAuthProviders: loadAuthProvidersMock,
  loginWithGithubAuth: loginWithGithubAuthMock,
}));

vi.mock("./services/processFeed", () => ({
  subscribeToProcessFeedEvents: subscribeToProcessFeedEventsMock,
}));

vi.mock("./services/runtimeEvents", () => ({
  subscribeToRuntimeEvents: subscribeToRuntimeEventsMock,
}));

vi.mock("./services/processDetail", () => ({
  rerunReleaseProcess: rerunReleaseProcessMock,
}));

vi.mock("./services/runtime", () => ({
  loadRuntimeHealth: loadRuntimeHealthMock,
  requestRepositoryInstantCheck: requestRepositoryInstantCheckMock,
  restartRuntime: restartRuntimeMock,
  startRuntime: startRuntimeMock,
  stopRuntime: stopRuntimeMock,
}));

import App from "./App";
import OverlayProvider, { useOverlay } from "./components/OverlayManager";

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

const COMPLETED_PROCESS = {
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

afterEach(() => {
  cleanup();
  document.body.style.overflow = "";
  vi.clearAllMocks();
});

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

  loadRepositoryInspectionMock.mockResolvedValue({
    repositories: [buildRepositoryInspectionEntry()],
  });

  createRepositoryProjectMock.mockResolvedValue({ repository_id: 7 });
  detectRepositoryProviderMock.mockResolvedValue(buildRepositoryProvider());
  loadAuthProvidersMock.mockResolvedValue([buildGithubAuthProvider()]);
  loadSecretSettingsMock.mockResolvedValue(buildSecretSettings());
  loginWithGithubAuthMock.mockResolvedValue(buildGithubAuthProvider());
  loadRuntimeHealthMock.mockResolvedValue({ status: "healthy" });
  reconnectRepositoryAuthMock.mockResolvedValue(undefined);
  requestRepositoryInstantCheckMock.mockResolvedValue(undefined);
  restartRuntimeMock.mockResolvedValue(undefined);
  rerunReleaseProcessMock.mockResolvedValue(undefined);
  saveSecretCredentialMock.mockResolvedValue(undefined);
  startRuntimeMock.mockResolvedValue(undefined);
  stopRuntimeMock.mockResolvedValue(undefined);
  stopRuntimeProcessFeedEventsMock.mockImplementation(() => undefined);
  subscribeToProcessFeedEventsMock.mockResolvedValue(
    stopRuntimeProcessFeedEventsMock,
  );
  stopRuntimeEventsMock.mockImplementation(() => undefined);
  subscribeToRuntimeEventsMock.mockResolvedValue(stopRuntimeEventsMock);
  validateUnityExecutablePathMock.mockResolvedValue(
    buildUnityExecutableValidation(),
  );
});

describe("App shell overlays", () => {
  it("opens the worker quick view and closes it on Escape without navigating away from the main feed", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });

    try {
      render(
        <OverlayProvider>
          <App />
        </OverlayProvider>,
      );

      const trigger = await screen.findByRole("button", {
        name: /Project workers active|Build target warning/i,
      });

      expect(trigger).toHaveAccessibleDescription(
        "Active workers: Worker Demo (Windows Build)",
      );

      trigger.focus();
      fireEvent.click(trigger);

      expect(
        await screen.findByRole("dialog", { name: "Project Workers" }),
      ).toBeInTheDocument();
      const openProjectWorkersButton = screen.getByRole("button", {
        name: "Open Project Workers",
      });

      expect(openProjectWorkersButton).toBeInTheDocument();

      await waitFor(() => {
        expect(openProjectWorkersButton).toHaveFocus();
      });

      fireEvent.keyDown(window, { key: "Escape" });

      await waitFor(() => {
        expect(
          screen.queryByRole("dialog", { name: "Project Workers" }),
        ).not.toBeInTheDocument();
      });

      await waitFor(() => {
        expect(trigger).toHaveFocus();
      });

      expect(
        screen.getByRole("button", { name: "Projects" }),
      ).toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: "Start" }),
      ).not.toBeInTheDocument();
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("closes the worker quick view from its close button and restores focus to the trigger", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });

    try {
      render(
        <OverlayProvider>
          <App />
        </OverlayProvider>,
      );

      const trigger = await screen.findByRole("button", {
        name: /Project workers active|Build target warning/i,
      });

      trigger.focus();
      fireEvent.click(trigger);

      const dialog = await screen.findByRole("dialog", {
        name: "Project Workers",
      });

      fireEvent.click(
        within(dialog).getByRole("button", { name: "Close overlay" }),
      );

      await waitFor(() => {
        expect(
          screen.queryByRole("dialog", { name: "Project Workers" }),
        ).not.toBeInTheDocument();
        expect(trigger).toHaveFocus();
      });
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("forces a GitHub browser relogin when a repository enters reauth required", async () => {
    loadRepositoryInspectionMock
      .mockResolvedValueOnce({
        repositories: [
          buildRepositoryInspectionEntry({
            auth_binding_status: "reauth_required",
            auth_status_message: "GitHub access must be refreshed.",
            credentials: {
              config_message: "Stored GitHub login metadata is valid.",
              config_status: "ready",
              credential_id: 101,
              kind: "git-http-github-host-login",
              name: "GitHub.com",
            },
          }),
        ],
      })
      .mockResolvedValueOnce({
        repositories: [
          buildRepositoryInspectionEntry({
            auth_binding_status: "bound_ready",
            auth_status_message: "GitHub access is connected.",
            credentials: {
              config_message: "Stored GitHub login metadata is valid.",
              config_status: "ready",
              credential_id: 101,
              kind: "git-http-github-host-login",
              name: "GitHub.com",
            },
          }),
        ],
      });

    render(
      <OverlayProvider>
        <App />
      </OverlayProvider>,
    );

    await waitFor(() => {
      expect(loginWithGithubAuthMock).toHaveBeenCalledWith({ force: true });
    });
    await waitFor(() => {
      expect(reconnectRepositoryAuthMock).toHaveBeenCalledWith(1, 101);
    });
  });

  it("confirms the runtime stop action before executing it from project workers", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });

    try {
      render(
        <OverlayProvider>
          <App />
        </OverlayProvider>,
      );

      const trigger = await screen.findByRole("button", {
        name: /Project workers active|Build target warning/i,
      });

      fireEvent.click(trigger);
      fireEvent.click(
        await screen.findByRole("button", { name: "Open Project Workers" }),
      );

      expect(
        await screen.findByRole("heading", { name: "Project Workers" }),
      ).toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "Stop" }));

      const dialog = await screen.findByRole("dialog", {
        name: "Stop runtime?",
      });

      expect(stopRuntimeMock).not.toHaveBeenCalled();

      fireEvent.click(
        within(dialog).getByRole("button", { name: "Stop runtime" }),
      );

      await waitFor(() => {
        expect(stopRuntimeMock).toHaveBeenCalledTimes(1);
      });
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("does not restart the runtime when the confirmation overlay is dismissed", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });

    try {
      render(
        <OverlayProvider>
          <App />
        </OverlayProvider>,
      );

      const trigger = await screen.findByRole("button", {
        name: /Project workers active|Build target warning/i,
      });

      fireEvent.click(trigger);
      fireEvent.click(
        await screen.findByRole("button", { name: "Open Project Workers" }),
      );

      expect(
        await screen.findByRole("heading", { name: "Project Workers" }),
      ).toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "Restart" }));

      const dialog = await screen.findByRole("dialog", {
        name: "Restart runtime?",
      });

      fireEvent.click(
        within(dialog).getByRole("button", { name: "Keep current state" }),
      );

      await waitFor(() => {
        expect(
          screen.queryByRole("dialog", { name: "Restart runtime?" }),
        ).not.toBeInTheDocument();
      });

      expect(restartRuntimeMock).not.toHaveBeenCalled();
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("queues bulk instant checks for selected workers after the operator confirms the batch", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });

    loadRepositoryInspectionMock.mockResolvedValue({
      repositories: [
        buildRepositoryInspectionEntry(),
        buildRepositoryInspectionEntry({
          build_targets: [
            {
              build_target_id: 22,
              diagnostic_message: "Needs a fresh host check.",
              diagnostic_status: "warning",
              enabled: true,
              repository_id: 2,
              repository_name: "Build Lab",
              runner_type: "host-native",
              target_name: "Linux Build",
              unity_build_method: "Builder.PerformLinuxBuild",
              unity_target_platform: "StandaloneLinux64",
              host_native_diagnostics: null,
            },
          ],
          enabled_build_target_count: 1,
          repository_id: 2,
          repository_name: "Build Lab",
          repo_url: "https://github.com/indiegabo/build-lab.git",
        }),
      ],
    });

    try {
      render(
        <OverlayProvider>
          <App />
        </OverlayProvider>,
      );

      const trigger = await screen.findByRole("button", {
        name: /Project workers active|Build target warning/i,
      });

      fireEvent.click(trigger);
      fireEvent.click(
        await screen.findByRole("button", { name: "Open Project Workers" }),
      );

      expect(
        await screen.findByRole("heading", { name: "Project Workers" }),
      ).toBeInTheDocument();

      fireEvent.click(
        screen.getByRole("button", { name: "Bulk instant check" }),
      );

      const selectionDialog = await screen.findByRole("dialog", {
        name: "Queue instant checks",
      });

      fireEvent.click(
        within(selectionDialog).getByRole("button", { name: /Worker Demo/i }),
      );
      fireEvent.click(
        within(selectionDialog).getByRole("button", { name: /Build Lab/i }),
      );
      fireEvent.click(
        within(selectionDialog).getByRole("button", {
          name: "Review queued checks",
        }),
      );

      const confirmDialog = await screen.findByRole("dialog", {
        name: "Queue instant checks?",
      });

      fireEvent.click(
        within(confirmDialog).getByRole("button", { name: "Queue checks" }),
      );

      await waitFor(() => {
        expect(requestRepositoryInstantCheckMock).toHaveBeenNthCalledWith(1, 1);
        expect(requestRepositoryInstantCheckMock).toHaveBeenNthCalledWith(2, 2);
      });

      expect(
        await screen.findByText("Instant checks queued for 2 projects."),
      ).toBeInTheDocument();
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("keeps the last known worker inventory visible and marks it stale when a refresh fails", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });

    loadRepositoryInspectionMock
      .mockResolvedValueOnce({
        repositories: [buildRepositoryInspectionEntry()],
      })
      .mockRejectedValue(new Error("Inspection offline"));

    try {
      render(
        <OverlayProvider>
          <App />
        </OverlayProvider>,
      );

      const trigger = await screen.findByRole("button", {
        name: /Project workers active/i,
      });

      fireEvent.click(trigger);
      fireEvent.click(
        await screen.findByRole("button", { name: "Open Project Workers" }),
      );

      expect(
        await screen.findByRole("heading", { name: "Project Workers" }),
      ).toBeInTheDocument();

      expect(await screen.findByText("Inspection offline")).toBeInTheDocument();
      expect(
        screen.getByText(
          "Showing the last known worker inventory while the shell recovers repository inspection.",
        ),
      ).toBeInTheDocument();
      expect(screen.getByText("Worker Demo")).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "Retry inventory" }),
      ).toBeInTheDocument();
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("updates the worker indicator from runtime.status_changed events without reloading repository inspection", async () => {
    let runtimeEventListener:
      | ((event: Record<string, unknown>) => void)
      | undefined;

    subscribeToRuntimeEventsMock.mockImplementation(async (listener) => {
      runtimeEventListener = listener as (event: Record<string, unknown>) => void;
      return stopRuntimeEventsMock;
    });

    render(
      <OverlayProvider>
        <App />
      </OverlayProvider>,
    );

    await screen.findByRole("button", {
      name: /Project workers active for 1 active project\./i,
    });

    expect(loadRepositoryInspectionMock).toHaveBeenCalledTimes(1);
    expect(runtimeEventListener).toBeDefined();

    await act(async () => {
      runtimeEventListener?.(
        buildRuntimeEvent({
          payload: { status: "shutting_down" },
          topic: "runtime.status_changed",
        }),
      );
    });

    await waitFor(() => {
      expect(
        screen.getByRole("button", {
          name: /Project workers are down for 1 active project while the runtime is shutting down\./i,
        }),
      ).toBeInTheDocument();
    });

    expect(loadRepositoryInspectionMock).toHaveBeenCalledTimes(1);
  });

  it("reloads repository inspection when runtime worker events arrive", async () => {
    let runtimeEventListener:
      | ((event: Record<string, unknown>) => void)
      | undefined;

    subscribeToRuntimeEventsMock.mockImplementation(async (listener) => {
      runtimeEventListener = listener as (event: Record<string, unknown>) => void;
      return stopRuntimeEventsMock;
    });

    render(
      <OverlayProvider>
        <App />
      </OverlayProvider>,
    );

    await screen.findByRole("button", {
      name: /Project workers active for 1 active project\./i,
    });

    expect(loadRepositoryInspectionMock).toHaveBeenCalledTimes(1);
    expect(runtimeEventListener).toBeDefined();

    await act(async () => {
      runtimeEventListener?.(
        buildRuntimeEvent({
          topic: "build.run_started",
        }),
      );
    });

    await waitFor(() => {
      expect(loadRepositoryInspectionMock).toHaveBeenCalledTimes(2);
    });
  });

  it("shows an in-app toast for automatic build failures and allows dismissal", async () => {
    let runtimeEventListener:
      | ((event: Record<string, unknown>) => void)
      | undefined;

    subscribeToRuntimeEventsMock.mockImplementation(async (listener) => {
      runtimeEventListener = listener as (event: Record<string, unknown>) => void;
      return stopRuntimeEventsMock;
    });

    render(
      <OverlayProvider>
        <App />
      </OverlayProvider>,
    );

    await screen.findByRole("button", {
      name: /Project workers active for 1 active project\./i,
    });

    await act(async () => {
      runtimeEventListener?.(
        buildRuntimeEvent({
          payload: { status: "failed" },
          summary: "Worker Demo build failed while the shell was visible.",
          topic: "build.run_finished",
        }),
      );
    });

    const notificationRail = await screen.findByRole("region", {
      name: "Runtime notifications",
    });

    expect(
      within(notificationRail).getByText("Automatic build failed"),
    ).toBeInTheDocument();
    expect(
      within(notificationRail).getByText(
        "Worker Demo build failed while the shell was visible.",
      ),
    ).toBeInTheDocument();

    fireEvent.click(
      within(notificationRail).getByRole("button", {
        name: "Dismiss Automatic build failed",
      }),
    );

    await waitFor(() => {
      expect(
        screen.queryByRole("region", { name: "Runtime notifications" }),
      ).not.toBeInTheDocument();
    });
  });

  it("shows an automatic release-queued toast without a linked process shortcut before the feed refresh lands", async () => {
    let runtimeEventListener:
      | ((event: Record<string, unknown>) => void)
      | undefined;

    subscribeToRuntimeEventsMock.mockImplementation(async (listener) => {
      runtimeEventListener = listener as (event: Record<string, unknown>) => void;
      return stopRuntimeEventsMock;
    });

    render(
      <OverlayProvider>
        <App />
      </OverlayProvider>,
    );

    await screen.findByRole("button", {
      name: /Project workers active for 1 active project\./i,
    });

    await act(async () => {
      runtimeEventListener?.(
        buildRuntimeEvent({
          payload: { status: "queued" },
          release_run_id: 101,
          summary: "Automatic release queued for Fresh Demo v0.2.0",
          topic: "automation.release_queued",
        }),
      );
    });

    const notificationRail = await screen.findByRole("region", {
      name: "Runtime notifications",
    });

    expect(
      within(notificationRail).getByText("Automatic release queued"),
    ).toBeInTheDocument();
    expect(
      within(notificationRail).getByText(
        "Automatic release queued for Fresh Demo v0.2.0",
      ),
    ).toBeInTheDocument();
    expect(
      within(notificationRail).queryByRole("button", {
        name: /Open process detail/i,
      }),
    ).not.toBeInTheDocument();
  });

  it("shows a publish completion toast with a linked process-detail shortcut", async () => {
    let runtimeEventListener:
      | ((event: Record<string, unknown>) => void)
      | undefined;

    subscribeToRuntimeEventsMock.mockImplementation(async (listener) => {
      runtimeEventListener = listener as (event: Record<string, unknown>) => void;
      return stopRuntimeEventsMock;
    });

    invokeMock.mockImplementation(async (command: string) => {
      switch (command) {
        case "main_window_pin_state":
          return false;
        case "process_feed":
          return buildProcessFeedPage({
            items: [COMPLETED_PROCESS],
            total_items: 1,
            total_pages: 1,
          });
        case "transition_window_focus":
        case "close_main_window":
          return undefined;
        case "set_main_window_pinned":
          return true;
        default:
          throw new Error(`Unexpected invoke command: ${command}`);
      }
    });

    render(
      <OverlayProvider>
        <App />
      </OverlayProvider>,
    );

    expect(
      await screen.findByRole("button", { name: "Open process detail #77" }),
    ).toBeInTheDocument();

    await act(async () => {
      runtimeEventListener?.(
        buildRuntimeEvent({
          payload: { status: "succeeded" },
          publish_run_id: 19,
          release_run_id: 77,
          summary: "Automatic publish succeeded for Worker Demo v0.1.0 (Itch stable)",
          topic: "publish.run_finished",
        }),
      );
    });

    const notificationRail = await screen.findByRole("region", {
      name: "Runtime notifications",
    });

    expect(
      within(notificationRail).getByText("Automatic publish finished"),
    ).toBeInTheDocument();
    expect(
      within(notificationRail).getByRole("button", {
        name: "Open process detail #77",
      }),
    ).toBeInTheDocument();

    fireEvent.click(
      within(notificationRail).getByRole("button", {
        name: "Open process detail #77",
      }),
    );

    expect(
      await screen.findByRole("button", { name: "Rerun process" }),
    ).toBeInTheDocument();
  });

  it("returns the process feed to page one when build events arrive on an older page", async () => {
    let processFeedEventListener:
      | ((event: Record<string, unknown>) => void)
      | undefined;

    subscribeToProcessFeedEventsMock.mockImplementation(async (listener) => {
      processFeedEventListener = listener as (event: Record<string, unknown>) => void;
      return stopRuntimeProcessFeedEventsMock;
    });

    invokeMock.mockImplementation(
      async (
        command: string,
        args?: { input?: { page?: number } },
      ) => {
        switch (command) {
          case "main_window_pin_state":
            return false;
          case "process_feed":
            return args?.input?.page === 2
              ? buildProcessFeedPage({
                  has_next_page: false,
                  has_previous_page: true,
                  items: [COMPLETED_PROCESS],
                  page: 2,
                  total_items: 2,
                  total_pages: 2,
                })
              : buildProcessFeedPage({
                  has_next_page: true,
                  items: [
                    {
                      ...COMPLETED_PROCESS,
                      git_tag: "v0.2.0",
                      release_run_id: 101,
                      repository_name: "Fresh Demo",
                    },
                  ],
                  page: 1,
                  total_items: 2,
                  total_pages: 2,
                });
          case "transition_window_focus":
          case "close_main_window":
            return undefined;
          case "set_main_window_pinned":
            return true;
          default:
            throw new Error(`Unexpected invoke command: ${command}`);
        }
      },
    );

    render(
      <OverlayProvider>
        <App />
      </OverlayProvider>,
    );

    expect(
      await screen.findByRole("button", { name: "Open process detail #101" }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Next" }));

    expect(
      await screen.findByRole("button", { name: "Open process detail #77" }),
    ).toBeInTheDocument();
    expect(processFeedEventListener).toBeDefined();

    await act(async () => {
      processFeedEventListener?.(
        buildRuntimeEvent({
          summary: "Automatic build started for Fresh Demo.",
          topic: "build.run_started",
        }),
      );
    });

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Open process detail #101" }),
      ).toBeInTheDocument();
    });

    expect(
      screen.queryByRole("button", { name: "Open process detail #77" }),
    ).not.toBeInTheDocument();
  });

  it("closes the top-most overlay on Back before leaving the current focus screen", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });

    try {
      render(
        <OverlayProvider>
          <App />
          <ShellOverlayHarness />
        </OverlayProvider>,
      );

      fireEvent.click(await screen.findByRole("button", { name: "Projects" }));

      expect(
        await screen.findByRole("heading", { name: "Project List" }),
      ).toBeInTheDocument();

      const overlayTrigger = screen.getByRole("button", {
        name: "Open shell test overlay",
      });
      overlayTrigger.focus();
      fireEvent.click(overlayTrigger);

      expect(
        await screen.findByRole("dialog", { name: "Shell test overlay" }),
      ).toBeInTheDocument();

      fireEvent.click(
        screen.getByRole("button", { name: "Back to main screen" }),
      );

      await waitFor(() => {
        expect(
          screen.queryByRole("dialog", { name: "Shell test overlay" }),
        ).not.toBeInTheDocument();
      });

      await waitFor(() => {
        expect(
          screen.getByRole("heading", { name: "Project List" }),
        ).toBeInTheDocument();
        expect(overlayTrigger).toHaveFocus();
      });

      expect(
        screen.queryByRole("button", { name: "Create project" }),
      ).not.toBeInTheDocument();
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("returns to the main feed when Back is pressed with no overlay open", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });

    try {
      render(
        <OverlayProvider>
          <App />
        </OverlayProvider>,
      );

      fireEvent.click(await screen.findByRole("button", { name: "Projects" }));

      expect(
        await screen.findByRole("heading", { name: "Project List" }),
      ).toBeInTheDocument();

      fireEvent.click(
        screen.getByRole("button", { name: "Back to main screen" }),
      );

      await waitFor(() => {
        expect(
          screen.getByRole("button", { name: "Create project" }),
        ).toBeInTheDocument();
      });

      expect(
        screen.queryByRole("heading", { name: "Project List" }),
      ).not.toBeInTheDocument();
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("confirms before leaving a dirty create-project draft and resumes when dismissal is canceled", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
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

      const nameInput = await screen.findByLabelText("Project name");
      fireEvent.change(nameInput, {
        target: { value: "Red Horizon" },
      });

      fireEvent.click(
        screen.getByRole("button", { name: "Back to main screen" }),
      );

      const dialog = await screen.findByRole("dialog", {
        name: "Discard project draft?",
      });

      fireEvent.click(
        within(dialog).getByRole("button", { name: "Continue editing" }),
      );

      await waitFor(() => {
        expect(
          screen.queryByRole("dialog", { name: "Discard project draft?" }),
        ).not.toBeInTheDocument();
        expect(screen.getByLabelText("Project name")).toHaveValue(
          "Red Horizon",
        );
      });

      fireEvent.click(
        screen.getByRole("button", { name: "Back to main screen" }),
      );

      const confirmDiscardDialog = await screen.findByRole("dialog", {
        name: "Discard project draft?",
      });

      fireEvent.click(
        within(confirmDiscardDialog).getByRole("button", {
          name: "Discard draft",
        }),
      );

      await waitFor(() => {
        expect(
          screen.getByRole("button", { name: "Create project" }),
        ).toBeInTheDocument();
      });

      expect(screen.queryByLabelText("Project name")).not.toBeInTheDocument();
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("resumes the create-project draft after returning from auth providers", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
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

      fireEvent.change(await screen.findByLabelText("Project name"), {
        target: { value: "Red Horizon" },
      });

      fireEvent.click(screen.getByRole("button", { name: "Next" }));

      fireEvent.change(
        await screen.findByPlaceholderText(
          "https://github.com/org/project.git",
        ),
        {
          target: { value: "https://github.com/indiegabo/red-horizon.git" },
        },
      );
      fireEvent.change(screen.getByRole("combobox"), {
        target: { value: "private" },
      });

      await waitFor(() => {
        expect(
          screen.getByRole("button", { name: "Open accounts" }),
        ).toBeInTheDocument();
      });

      fireEvent.click(screen.getByRole("button", { name: "Open accounts" }));

      expect(
        await screen.findByRole("button", { name: "Back to project creation" }),
      ).toBeInTheDocument();

      fireEvent.click(
        screen.getByRole("button", { name: "Back to project creation" }),
      );

      await waitFor(() => {
        expect(
          screen.getByPlaceholderText("https://github.com/org/project.git"),
        ).toHaveValue("https://github.com/indiegabo/red-horizon.git");
        expect(screen.getByRole("combobox")).toHaveValue("private");
      });

      expect(screen.queryByLabelText("Project name")).not.toBeInTheDocument();
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("binds the auth-provider result back into the create-project access step", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
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

      fireEvent.change(await screen.findByLabelText("Project name"), {
        target: { value: "Red Horizon" },
      });

      fireEvent.click(screen.getByRole("button", { name: "Next" }));

      fireEvent.change(
        await screen.findByPlaceholderText(
          "https://github.com/org/project.git",
        ),
        {
          target: { value: "https://github.com/indiegabo/red-horizon.git" },
        },
      );
      fireEvent.change(screen.getByRole("combobox"), {
        target: { value: "private" },
      });

      fireEvent.click(
        await screen.findByRole("button", { name: "Open accounts" }),
      );

      fireEvent.click(
        await screen.findByRole("button", {
          name: /Review (connection|reconnect)/i,
        }),
      );

      const dialog = await screen.findByRole("dialog", {
        name: "GitHub connection",
      });

      fireEvent.click(within(dialog).getByRole("button", { name: "Continue" }));
      fireEvent.click(
        within(dialog).getByRole("button", { name: "Reconnect with browser" }),
      );

      await waitFor(() => {
        expect(loginWithGithubAuthMock).toHaveBeenCalledWith({ force: true });
      });

      fireEvent.click(
        await screen.findByRole("button", { name: "Back to project creation" }),
      );

      await waitFor(() => {
        expect(
          screen.getByRole("combobox", { name: "Repository credential" }),
        ).toHaveValue("101");
      });

      expect(
        screen.getByText(
          "GitHub browser reconnect completed. 1 repository project is currently bound to it. The connected credential is now selected for this project draft.",
        ),
      ).toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "Next" }));

      expect(
        await screen.findByRole("heading", { name: "Build Targets" }),
      ).toBeInTheDocument();
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("closes the window directly even while another overlay is open", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });

    try {
      render(
        <OverlayProvider>
          <App />
          <ShellOverlayHarness />
        </OverlayProvider>,
      );

      fireEvent.click(await screen.findByRole("button", { name: "Projects" }));

      expect(
        await screen.findByRole("heading", { name: "Project List" }),
      ).toBeInTheDocument();

      fireEvent.click(
        screen.getByRole("button", { name: "Open shell test overlay" }),
      );

      expect(
        await screen.findByRole("dialog", { name: "Shell test overlay" }),
      ).toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "Close window" }));

      await waitFor(() => {
        expect(invokeMock).toHaveBeenCalledWith("close_main_window");
      });
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("closes the window directly from a focus screen", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });

    try {
      render(
        <OverlayProvider>
          <App />
        </OverlayProvider>,
      );

      fireEvent.click(await screen.findByRole("button", { name: "Projects" }));

      expect(
        await screen.findByRole("heading", { name: "Project List" }),
      ).toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "Close window" }));

      await waitFor(() => {
        expect(invokeMock).toHaveBeenCalledWith("close_main_window");
      });

      expect(
        screen.getByRole("heading", { name: "Project List" }),
      ).toBeInTheDocument();
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("closes the window immediately from the main screen", async () => {
    render(
      <OverlayProvider>
        <App />
      </OverlayProvider>,
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "Close window" }),
    );

    expect(
      screen.queryByRole("dialog", { name: "Close the HGP window?" }),
    ).not.toBeInTheDocument();

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("close_main_window");
    });
  });

  it("confirms a rerun from process detail and returns to the main feed", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });

    invokeMock.mockImplementation(async (command: string) => {
      switch (command) {
        case "main_window_pin_state":
          return false;
        case "process_feed":
          return {
            ...EMPTY_PROCESS_FEED_PAGE,
            items: [COMPLETED_PROCESS],
            total_items: 1,
            total_pages: 1,
          };
        case "transition_window_focus":
        case "close_main_window":
          return undefined;
        case "set_main_window_pinned":
          return true;
        default:
          throw new Error(`Unexpected invoke command: ${command}`);
      }
    });

    try {
      render(
        <OverlayProvider>
          <App />
        </OverlayProvider>,
      );

      fireEvent.click(
        await screen.findByRole("button", {
          name: "Open process detail #77",
        }),
      );

      expect(
        await screen.findByRole("button", { name: "Rerun process" }),
      ).toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: "Create project" }),
      ).not.toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "Rerun process" }));

      const dialog = await screen.findByRole("dialog", {
        name: "Rerun process?",
      });

      fireEvent.click(
        within(dialog).getByRole("button", { name: "Rerun process" }),
      );

      await waitFor(() => {
        expect(rerunReleaseProcessMock).toHaveBeenCalledWith(77);
      });

      await waitFor(() => {
        expect(
          screen.getByRole("button", { name: "Create project" }),
        ).toBeInTheDocument();
      });

      expect(
        screen.queryByRole("button", { name: "Rerun process" }),
      ).not.toBeInTheDocument();
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("skips shell blank-frame waiting when reduced motion is requested", async () => {
    const requestAnimationFrameSpy = vi.spyOn(window, "requestAnimationFrame");

    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      writable: true,
      value: vi.fn().mockImplementation((query: string) => ({
        matches: query === "(prefers-reduced-motion: reduce)",
        media: query,
        onchange: null,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        addListener: vi.fn(),
        removeListener: vi.fn(),
        dispatchEvent: vi.fn(),
      })),
    });

    try {
      render(
        <OverlayProvider>
          <App />
        </OverlayProvider>,
      );

      fireEvent.click(await screen.findByRole("button", { name: "Projects" }));

      expect(
        await screen.findByRole("heading", { name: "Project List" }),
      ).toBeInTheDocument();

      expect(requestAnimationFrameSpy).not.toHaveBeenCalled();
      expect(invokeMock).toHaveBeenCalledWith("transition_window_focus", {
        target: "focus",
      });
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });
});

function ShellOverlayHarness() {
  const { openOverlay } = useOverlay();

  return (
    <button
      onClick={() => {
        void openOverlay(ShellTestOverlay);
      }}
      type="button"
    >
      Open shell test overlay
    </button>
  );
}

function buildGithubAuthProvider() {
  return {
    bound_repository_count: 1,
    credential_id: 101,
    credential_name: "GitHub.com",
    instance_url: "https://github.com",
    label: "GitHub",
    provider_id: "github",
    status: "connected",
    status_message: "GitHub login connected.",
  };
}

function buildRepositoryProvider() {
  return {
    instance_url: "https://github.com",
    normalized_url: "https://github.com/indiegabo/red-horizon.git",
    provider_id: "github",
    provider_label: "GitHub",
    supports_interactive_login: true,
  };
}

function buildSecretSettings() {
  return {
    credentials: [
      {
        config_summary: {
          message: "Stored GitHub login metadata is valid.",
          missing_required_keys: [],
          status: "ready",
          top_level_keys: [
            "auth_mode",
            "credential_helper",
            "instance_url",
            "provider",
          ],
        },
        created_at: "2026-05-19T00:00:00Z",
        credential_id: 101,
        kind: "git-http-github-host-login",
        name: "GitHub.com",
        storage_model: "sqlite-config-json-and-keyring-references",
        updated_at: "2026-05-19T00:00:00Z",
      },
    ],
    storage_model: "sqlite-config-json-and-keyring-references",
    supported_credential_kinds: [
      "git-http-basic",
      "git-http-bearer",
      "git-http-github-host-login",
      "itch-api-key",
    ],
    warnings: [],
  };
}

function buildUnityExecutableValidation() {
  return {
    additional_argument_count: 0,
    environment_variable_count: 0,
    message: "Unity executable is ready.",
    runner_family: "host-native",
    status: "ready",
    unity_executable_exists: true,
    unity_executable_is_file: true,
    unity_executable_path:
      "C:/Program Files/Unity/Hub/Editor/6000.0.23f1/Editor/Unity.exe",
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
    credentials: {
      config_message: string;
      config_status: string;
      credential_id: number;
      kind: string;
      name: string;
    } | null;
    default_branch: string;
    enabled_build_target_count: number;
    engine_kind: string;
    enabled: boolean;
    last_seen_tag: string | null;
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
    source_provider_id: string | null;
    visibility_status: string;
    workspace_root_override: string | null;
  }> = {},
) {
  return {
    artifacts_root_override: null,
    auth_binding_status: "connected",
    auth_last_verified_at: "2026-05-19T00:00:00Z",
    auth_requirement_status: "required",
    auth_status_message: "GitHub access is connected.",
    build_targets: [
      {
        build_target_id: 11,
        diagnostic_message: "",
        diagnostic_status: "ready",
        enabled: true,
        repository_id: 1,
        repository_name: "Worker Demo",
        runner_type: "host-native",
        target_name: "Windows Build",
        unity_build_method: "Builder.PerformBuild",
        unity_target_platform: "StandaloneWindows64",
        host_native_diagnostics: null,
      },
    ],
    credentials: null,
    default_branch: "main",
    enabled_build_target_count: 1,
    engine_kind: "unity",
    enabled: true,
    last_seen_tag: "v0.1.0",
    pending_release_count: 0,
    polling_interval_seconds: 30,
    publish_targets: [],
    queued_build_runs: 0,
    queued_publish_runs: 0,
    release_queue: [],
    repo_url: "https://github.com/indiegabo/worker-demo.git",
    repository_id: 1,
    repository_name: "Worker Demo",
    running_build_runs: 0,
    running_publish_runs: 0,
    source_instance_url: "https://github.com",
    source_provider_id: "github",
    visibility_status: "private",
    workspace_root_override: null,
    ...overrides,
  };
}

function buildRuntimeEvent(
  overrides: Partial<{
    build_run_id: number | null;
    event_id: string;
    occurred_at_unix_millis: number;
    origin: string;
    payload: Record<string, unknown>;
    publish_run_id: number | null;
    release_run_id: number | null;
    repository_id: number | null;
    severity: string;
    summary: string;
    topic: string;
    user_requested: boolean;
  }> = {},
) {
  return {
    build_run_id: null,
    event_id: "evt_1",
    occurred_at_unix_millis: 1,
    origin: "desktop-shell",
    payload: {},
    publish_run_id: null,
    release_run_id: null,
    repository_id: 1,
    severity: "info",
    summary: "Runtime event",
    topic: "runtime.status_changed",
    user_requested: false,
    ...overrides,
  };
}

function buildProcessFeedPage(
  overrides: Partial<{
    generated_at: string;
    has_next_page: boolean;
    has_previous_page: boolean;
    items: Array<typeof COMPLETED_PROCESS>;
    page: number;
    page_size: number;
    total_items: number;
    total_pages: number;
  }> = {},
) {
  return {
    ...EMPTY_PROCESS_FEED_PAGE,
    items: [] as Array<typeof COMPLETED_PROCESS>,
    ...overrides,
  };
}

function ShellTestOverlay({
  onResolve,
}: {
  onResolve?: (value?: null) => void;
}) {
  return (
    <div aria-label="Shell test overlay" role="dialog">
      <button onClick={() => onResolve?.(null)} type="button">
        Close shell test overlay
      </button>
    </div>
  );
}
