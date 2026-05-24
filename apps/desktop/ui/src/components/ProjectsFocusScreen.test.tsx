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
import { ProjectsFocusScreen } from "./ProjectsFocusScreen";

const { loadRepositoryInspectionMock, dispatchOnDemandReleaseProcessMock } =
  vi.hoisted(() => ({
    loadRepositoryInspectionMock: vi.fn(),
    dispatchOnDemandReleaseProcessMock: vi.fn(),
  }));

vi.mock("../services/projects", () => ({
  dispatchOnDemandReleaseProcess: dispatchOnDemandReleaseProcessMock,
  loadRepositoryInspection: loadRepositoryInspectionMock,
}));

afterEach(() => {
  cleanup();
  document.body.style.overflow = "";
  vi.clearAllMocks();
});

beforeEach(() => {
  loadRepositoryInspectionMock.mockResolvedValue(buildRepositoryInspection());
  dispatchOnDemandReleaseProcessMock.mockResolvedValue({
    git_tag: "v1.2.3",
    id: 7,
    repository_id: 2,
    status: "queued",
  });
});

describe("ProjectsFocusScreen", () => {
  it("renders an in-panel error state and supports retrying the repository load", async () => {
    loadRepositoryInspectionMock.mockReset();
    loadRepositoryInspectionMock
      .mockRejectedValueOnce(new Error("Inspection offline"))
      .mockResolvedValueOnce(buildRepositoryInspection());

    render(
      <OverlayProvider>
        <ProjectsFocusScreen onOpenProject={vi.fn()} />
      </OverlayProvider>,
    );

    expect(
      await screen.findByText("Could not load projects."),
    ).toBeInTheDocument();
    expect(screen.getByText("Inspection offline")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry load" }));

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Quick view for Worker Demo" }),
      ).toBeEnabled();
    });
  });

  it("keeps the current inventory visible while a refresh is in progress", async () => {
    const refreshDeferred =
      createDeferred<ReturnType<typeof buildRepositoryInspection>>();

    loadRepositoryInspectionMock.mockReset();
    loadRepositoryInspectionMock
      .mockResolvedValueOnce(buildRepositoryInspection())
      .mockImplementationOnce(() => refreshDeferred.promise);

    render(
      <OverlayProvider>
        <ProjectsFocusScreen onOpenProject={vi.fn()} />
      </OverlayProvider>,
    );

    await screen.findByRole("button", { name: "Quick view for Worker Demo" });

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    expect(
      screen.getByRole("button", { name: "Refreshing..." }),
    ).toBeDisabled();
    expect(
      screen.getByText(/Refreshing repository inventory/i),
    ).toBeInTheDocument();
    expect(screen.getByText("Worker Demo")).toBeInTheDocument();

    refreshDeferred.resolve(buildRepositoryInspection());

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled();
    });
  });

  it("opens the quick-open picker and returns the selected project", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });
    const onOpenProject = vi.fn();

    try {
      render(
        <OverlayProvider>
          <ProjectsFocusScreen onOpenProject={onOpenProject} />
        </OverlayProvider>,
      );

      const browseButton = await screen.findByRole("button", {
        name: "Browse",
      });

      await waitFor(() => {
        expect(browseButton).toBeEnabled();
      });

      browseButton.focus();
      fireEvent.click(browseButton);

      const dialog = await screen.findByRole("dialog", {
        name: "Open project",
      });

      fireEvent.click(
        within(dialog).getByRole("button", {
          name: /Worker Demo/i,
        }),
      );

      await waitFor(() => {
        expect(onOpenProject).toHaveBeenCalledWith(1, "Worker Demo");
      });

      await waitFor(() => {
        expect(
          screen.queryByRole("dialog", { name: "Open project" }),
        ).not.toBeInTheDocument();
        expect(browseButton).toHaveFocus();
      });
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("moves focus between the quick-open input and project cards with ArrowDown and ArrowUp", async () => {
    loadRepositoryInspectionMock.mockResolvedValueOnce({
      repositories: [
        ...buildRepositoryInspection().repositories,
        {
          ...buildRepositoryInspection().repositories[0],
          default_branch: "develop",
          enabled: false,
          repository_id: 2,
          repository_name: "Build Lab",
          repo_url: "https://github.com/indiegabo/build-lab.git",
        },
      ],
    });

    render(
      <OverlayProvider>
        <ProjectsFocusScreen onOpenProject={vi.fn()} />
      </OverlayProvider>,
    );

    const quickOpenInput = await screen.findByRole("textbox", {
      name: "Quick open",
    });
    await screen.findByRole("button", {
      name: "Open project Worker Demo",
    });
    await screen.findByRole("button", {
      name: "Open project Build Lab",
    });

    quickOpenInput.focus();
    fireEvent.keyDown(quickOpenInput, { key: "ArrowDown" });

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Open project Worker Demo" }),
      ).toHaveFocus();
    });

    quickOpenInput.focus();
    fireEvent.keyDown(quickOpenInput, { key: "ArrowUp" });

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Open project Build Lab" }),
      ).toHaveFocus();
    });
  });

  it("supports ArrowUp, ArrowDown, Home, and End navigation across project cards", async () => {
    loadRepositoryInspectionMock.mockResolvedValueOnce({
      repositories: [
        ...buildRepositoryInspection().repositories,
        {
          ...buildRepositoryInspection().repositories[0],
          default_branch: "develop",
          enabled: false,
          repository_id: 2,
          repository_name: "Build Lab",
          repo_url: "https://github.com/indiegabo/build-lab.git",
        },
        {
          ...buildRepositoryInspection().repositories[0],
          default_branch: "release",
          repository_id: 3,
          repository_name: "Release Forge",
          repo_url: "https://github.com/indiegabo/release-forge.git",
        },
      ],
    });

    render(
      <OverlayProvider>
        <ProjectsFocusScreen onOpenProject={vi.fn()} />
      </OverlayProvider>,
    );

    const workerDemoButton = await screen.findByRole("button", {
      name: "Open project Worker Demo",
    });
    const buildLabButton = screen.getByRole("button", {
      name: "Open project Build Lab",
    });
    const releaseForgeButton = screen.getByRole("button", {
      name: "Open project Release Forge",
    });

    workerDemoButton.focus();
    fireEvent.keyDown(workerDemoButton, { key: "ArrowDown" });
    await waitFor(() => {
      expect(buildLabButton).toHaveFocus();
    });

    fireEvent.keyDown(buildLabButton, { key: "End" });
    await waitFor(() => {
      expect(releaseForgeButton).toHaveFocus();
    });

    fireEvent.keyDown(releaseForgeButton, { key: "ArrowUp" });
    await waitFor(() => {
      expect(buildLabButton).toHaveFocus();
    });

    fireEvent.keyDown(buildLabButton, { key: "Home" });
    await waitFor(() => {
      expect(workerDemoButton).toHaveFocus();
    });
  });

  it("returns focus from a project card to the quick-open input with Escape", async () => {
    loadRepositoryInspectionMock.mockResolvedValueOnce({
      repositories: [
        ...buildRepositoryInspection().repositories,
        {
          ...buildRepositoryInspection().repositories[0],
          default_branch: "develop",
          enabled: false,
          repository_id: 2,
          repository_name: "Build Lab",
          repo_url: "https://github.com/indiegabo/build-lab.git",
        },
      ],
    });

    render(
      <OverlayProvider>
        <ProjectsFocusScreen onOpenProject={vi.fn()} />
      </OverlayProvider>,
    );

    const quickOpenInput = await screen.findByRole("textbox", {
      name: "Quick open",
    });
    const workerDemoButton = await screen.findByRole("button", {
      name: "Open project Worker Demo",
    });

    workerDemoButton.focus();
    fireEvent.keyDown(workerDemoButton, { key: "Escape" });

    await waitFor(() => {
      expect(quickOpenInput).toHaveFocus();
    });
  });

  it("uses repository-specific accessible names for quick-view buttons", async () => {
    loadRepositoryInspectionMock.mockResolvedValueOnce({
      repositories: [
        ...buildRepositoryInspection().repositories,
        {
          ...buildRepositoryInspection().repositories[0],
          default_branch: "develop",
          enabled: false,
          repository_id: 2,
          repository_name: "Build Lab",
          repo_url: "https://github.com/indiegabo/build-lab.git",
        },
      ],
    });

    render(
      <OverlayProvider>
        <ProjectsFocusScreen onOpenProject={vi.fn()} />
      </OverlayProvider>,
    );

    expect(
      await screen.findByRole("button", { name: "Quick view for Worker Demo" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Quick view for Build Lab" }),
    ).toBeInTheDocument();
  });

  it("opens an exact quick-open match with Enter", async () => {
    const onOpenProject = vi.fn();

    render(
      <OverlayProvider>
        <ProjectsFocusScreen onOpenProject={onOpenProject} />
      </OverlayProvider>,
    );

    const quickOpenInput = await screen.findByRole("textbox", {
      name: "Quick open",
    });

    fireEvent.change(quickOpenInput, { target: { value: "Worker Demo" } });
    fireEvent.keyDown(quickOpenInput, { key: "Enter" });

    await waitFor(() => {
      expect(onOpenProject).toHaveBeenCalledWith(1, "Worker Demo");
    });
  });

  it("searches and previews local workspace projects by their source path", async () => {
    const onOpenProject = vi.fn();

    loadRepositoryInspectionMock.mockResolvedValueOnce({
      repositories: [
        ...buildRepositoryInspection().repositories,
        {
          ...buildRepositoryInspection().repositories[0],
          auth_binding_status: "not_required",
          auth_requirement_status: "not_required",
          auth_status_message: "Local workspace projects do not need auth.",
          local_path: "C:/projects/local-forge",
          pending_release_count: 1,
          repo_url: "C:/projects/local-forge",
          repository_id: 2,
          repository_name: "Local Forge",
          source_mode: "local_workspace",
          source_provider_id: null,
          visibility_status: "unknown",
        },
      ],
    });

    render(
      <OverlayProvider>
        <ProjectsFocusScreen onOpenProject={onOpenProject} />
      </OverlayProvider>,
    );

    const quickOpenInput = await screen.findByRole("textbox", {
      name: "Quick open",
    });

    fireEvent.change(quickOpenInput, {
      target: { value: "C:/projects/local-forge" },
    });

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Open project Local Forge" }),
      ).toBeInTheDocument();
    });
    expect(
      screen.queryByRole("button", { name: "Open project Worker Demo" }),
    ).not.toBeInTheDocument();

    fireEvent.keyDown(quickOpenInput, { key: "Enter" });

    await waitFor(() => {
      expect(onOpenProject).toHaveBeenCalledWith(2, "Local Forge");
    });

    fireEvent.click(
      screen.getByRole("button", { name: "Quick view for Local Forge" }),
    );

    const dialog = await screen.findByRole("dialog", {
      name: "Local Forge",
    });

    expect(
      within(dialog).getByText("Local workspace · C:/projects/local-forge"),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByText("C:/projects/local-forge"),
    ).toBeInTheDocument();
  });

  it("opens the project quick view and escalates to the project editor on demand", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });
    const onOpenProject = vi.fn();

    try {
      render(
        <OverlayProvider>
          <ProjectsFocusScreen onOpenProject={onOpenProject} />
        </OverlayProvider>,
      );

      const quickViewButton = await screen.findByRole("button", {
        name: "Quick view for Worker Demo",
      });

      quickViewButton.focus();
      fireEvent.click(quickViewButton);

      const dialog = await screen.findByRole("dialog", {
        name: "Worker Demo",
      });
      const openProjectButton = within(dialog).getByRole("button", {
        name: "Open Project",
      });

      expect(
        within(dialog).getByText("Automation Snapshot"),
      ).toBeInTheDocument();

      await waitFor(() => {
        expect(openProjectButton).toHaveFocus();
      });

      fireEvent.click(openProjectButton);

      await waitFor(() => {
        expect(onOpenProject).toHaveBeenCalledWith(1, "Worker Demo");
      });

      await waitFor(() => {
        expect(
          screen.queryByRole("dialog", { name: "Worker Demo" }),
        ).not.toBeInTheDocument();
        expect(quickViewButton).toHaveFocus();
      });
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("uses shared summary strips for project cards and the quick-view overlay", async () => {
    render(
      <OverlayProvider>
        <ProjectsFocusScreen onOpenProject={vi.fn()} />
      </OverlayProvider>,
    );

    const projectCard = await screen.findByRole("button", {
      name: "Open project Worker Demo",
    });

    expect(
      projectCard.querySelector(".project-list-card__summary-strip"),
    ).not.toBeNull();

    fireEvent.click(
      screen.getByRole("button", {
        name: "Quick view for Worker Demo",
      }),
    );

    const dialog = await screen.findByRole("dialog", {
      name: "Worker Demo",
    });

    expect(
      dialog.querySelector(".project-quick-view__summary-strip"),
    ).not.toBeNull();
  });

  it("dismisses the project quick view with Escape and restores focus to its trigger", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });

    try {
      render(
        <OverlayProvider>
          <ProjectsFocusScreen onOpenProject={vi.fn()} />
        </OverlayProvider>,
      );

      const quickViewButton = await screen.findByRole("button", {
        name: "Quick view for Worker Demo",
      });

      quickViewButton.focus();
      fireEvent.click(quickViewButton);

      const dialog = await screen.findByRole("dialog", {
        name: "Worker Demo",
      });

      fireEvent.keyDown(dialog, { key: "Escape" });

      await waitFor(() => {
        expect(
          screen.queryByRole("dialog", { name: "Worker Demo" }),
        ).not.toBeInTheDocument();
        expect(quickViewButton).toHaveFocus();
      });
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });

  it("dismisses the project quick view from its close button and restores focus to its trigger", async () => {
    const requestAnimationFrameSpy = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      });

    try {
      render(
        <OverlayProvider>
          <ProjectsFocusScreen onOpenProject={vi.fn()} />
        </OverlayProvider>,
      );

      const quickViewButton = await screen.findByRole("button", {
        name: "Quick view for Worker Demo",
      });

      quickViewButton.focus();
      fireEvent.click(quickViewButton);

      const dialog = await screen.findByRole("dialog", {
        name: "Worker Demo",
      });

      fireEvent.click(
        within(dialog).getByRole("button", { name: "Close overlay" }),
      );

      await waitFor(() => {
        expect(
          screen.queryByRole("dialog", { name: "Worker Demo" }),
        ).not.toBeInTheDocument();
        expect(quickViewButton).toHaveFocus();
      });
    } finally {
      requestAnimationFrameSpy.mockRestore();
    }
  });
});

function buildRepositoryInspection() {
  return {
    repositories: [
      {
        artifacts_root_override: null,
        auth_binding_status: "connected",
        auth_last_verified_at: "2026-05-19T00:00:00Z",
        auth_requirement_status: "required",
        auth_status_message: "GitHub access is connected.",
        build_targets: [
          {
            additional_argument_count: 0,
            build_target_id: 11,
            diagnostic_message: "Ready for host-native execution.",
            diagnostic_status: "ready",
            enabled: true,
            environment_variable_count: 0,
            host_native_diagnostics: null,
            name: "Windows Build",
            repository_id: 1,
            repository_name: "Worker Demo",
            runner_type: "host-native",
            target_name: "Windows Build",
            unity_build_method: "Builder.PerformBuild",
            unity_target_platform: "StandaloneWindows64",
          },
        ],
        credentials: null,
        default_branch: "main",
        enabled: true,
        enabled_build_target_count: 1,
        engine_kind: "unity",
        last_seen_tag: "v0.1.0",
        local_path: null,
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
        source_mode: "managed_repository",
        source_instance_url: "https://github.com",
        source_provider_id: "github",
        visibility_status: "private",
        workspace_root_override: null,
      },
    ],
  };
}

function createDeferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((nextResolve, nextReject) => {
    resolve = nextResolve;
    reject = nextReject;
  });

  return { promise, reject, resolve };
}
