import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const {
  getCurrentWindowMock,
  invokeMock,
  loadRepositoryInspectionMock,
  loginWithGithubAuthMock,
  loadRuntimeHealthMock,
  reconnectRepositoryAuthMock,
  rerunReleaseProcessMock,
  requestRepositoryInstantCheckMock,
  restartRuntimeMock,
  startDraggingMock,
  startRuntimeMock,
  stopRuntimeMock,
  stopRuntimeProcessFeedEventsMock,
  subscribeToProcessFeedEventsMock,
  stopRuntimeEventsMock,
  subscribeToRuntimeEventsMock,
} = vi.hoisted(() => ({
  getCurrentWindowMock: vi.fn(),
  invokeMock: vi.fn(),
  loadRepositoryInspectionMock: vi.fn(),
  loginWithGithubAuthMock: vi.fn(),
  loadRuntimeHealthMock: vi.fn(),
  reconnectRepositoryAuthMock: vi.fn(),
  rerunReleaseProcessMock: vi.fn(),
  requestRepositoryInstantCheckMock: vi.fn(),
  restartRuntimeMock: vi.fn(),
  startDraggingMock: vi.fn(() => Promise.resolve()),
  startRuntimeMock: vi.fn(),
  stopRuntimeMock: vi.fn(),
  stopRuntimeProcessFeedEventsMock: vi.fn(),
  subscribeToProcessFeedEventsMock: vi.fn(),
  stopRuntimeEventsMock: vi.fn(),
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

  loginWithGithubAuthMock.mockResolvedValue(buildGithubAuthProvider());
  loadRuntimeHealthMock.mockResolvedValue({ status: "healthy" });
  reconnectRepositoryAuthMock.mockResolvedValue(undefined);
  requestRepositoryInstantCheckMock.mockResolvedValue(undefined);
  restartRuntimeMock.mockResolvedValue(undefined);
  rerunReleaseProcessMock.mockResolvedValue(undefined);
  startRuntimeMock.mockResolvedValue(undefined);
  stopRuntimeMock.mockResolvedValue(undefined);
  stopRuntimeProcessFeedEventsMock.mockImplementation(() => undefined);
  subscribeToProcessFeedEventsMock.mockResolvedValue(
    stopRuntimeProcessFeedEventsMock,
  );
  stopRuntimeEventsMock.mockImplementation(() => undefined);
  subscribeToRuntimeEventsMock.mockResolvedValue(stopRuntimeEventsMock);
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

      expect(trigger).toHaveAttribute(
        "title",
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
        screen.getByRole("button", { name: "Projetos" }),
      ).toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: "Start" }),
      ).not.toBeInTheDocument();
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

      fireEvent.click(await screen.findByRole("button", { name: "Projetos" }));

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
        screen.getByRole("button", { name: "Voltar para a tela principal" }),
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
        screen.queryByRole("button", { name: "Criar novo projeto" }),
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

      fireEvent.click(await screen.findByRole("button", { name: "Projetos" }));

      expect(
        await screen.findByRole("heading", { name: "Project List" }),
      ).toBeInTheDocument();

      fireEvent.click(
        screen.getByRole("button", { name: "Voltar para a tela principal" }),
      );

      await waitFor(() => {
        expect(
          screen.getByRole("button", { name: "Criar novo projeto" }),
        ).toBeInTheDocument();
      });

      expect(
        screen.queryByRole("heading", { name: "Project List" }),
      ).not.toBeInTheDocument();
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

      fireEvent.click(await screen.findByRole("button", { name: "Projetos" }));

      expect(
        await screen.findByRole("heading", { name: "Project List" }),
      ).toBeInTheDocument();

      fireEvent.click(
        screen.getByRole("button", { name: "Open shell test overlay" }),
      );

      expect(
        await screen.findByRole("dialog", { name: "Shell test overlay" }),
      ).toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "Fechar janela" }));

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

      fireEvent.click(await screen.findByRole("button", { name: "Projetos" }));

      expect(
        await screen.findByRole("heading", { name: "Project List" }),
      ).toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "Fechar janela" }));

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
      await screen.findByRole("button", { name: "Fechar janela" }),
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
          name: "Abrir detalhe do processo #77",
        }),
      );

      expect(
        await screen.findByRole("button", { name: "Rerun process" }),
      ).toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: "Criar novo projeto" }),
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
          screen.getByRole("button", { name: "Criar novo projeto" }),
        ).toBeInTheDocument();
      });

      expect(
        screen.queryByRole("button", { name: "Rerun process" }),
      ).not.toBeInTheDocument();
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
