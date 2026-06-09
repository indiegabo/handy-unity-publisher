import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import OverlayProvider from "./OverlayManager";
import { RepositoryProjectDetail } from "./RepositoryProjectDetail";

const {
  connectRepositoryAuthMock,
  detectRepositoryProviderMock,
  disconnectRepositoryAuthMock,
  loadDefaultProjectWorkspaceRootMock,
  loadAuthProvidersMock,
  loadRepositoryInspectionMock,
  loadRepositoryProjectDetailMock,
  loadSecretSettingsMock,
  loadUnityAdapterSettingsMock,
  loginWithGithubAuthMock,
  reconnectRepositoryAuthMock,
  removeRepositoryProjectMock,
  saveSecretCredentialMock,
  updateRepositoryProjectMock,
  validateUnityExecutablePathMock,
} = vi.hoisted(() => ({
  connectRepositoryAuthMock: vi.fn(),
  detectRepositoryProviderMock: vi.fn(),
  disconnectRepositoryAuthMock: vi.fn(),
  loadDefaultProjectWorkspaceRootMock: vi.fn(),
  loadAuthProvidersMock: vi.fn(),
  loadRepositoryInspectionMock: vi.fn(),
  loadRepositoryProjectDetailMock: vi.fn(),
  loadSecretSettingsMock: vi.fn(),
  loadUnityAdapterSettingsMock: vi.fn(),
  loginWithGithubAuthMock: vi.fn(),
  reconnectRepositoryAuthMock: vi.fn(),
  removeRepositoryProjectMock: vi.fn(),
  saveSecretCredentialMock: vi.fn(),
  updateRepositoryProjectMock: vi.fn(),
  validateUnityExecutablePathMock: vi.fn(),
}));

vi.mock("../services/projects", () => ({
  connectRepositoryAuth: connectRepositoryAuthMock,
  detectRepositoryProvider: detectRepositoryProviderMock,
  disconnectRepositoryAuth: disconnectRepositoryAuthMock,
  loadDefaultProjectWorkspaceRoot: loadDefaultProjectWorkspaceRootMock,
  loadRepositoryInspection: loadRepositoryInspectionMock,
  loadRepositoryProjectDetail: loadRepositoryProjectDetailMock,
  loadSecretSettings: loadSecretSettingsMock,
  loadUnityAdapterSettings: loadUnityAdapterSettingsMock,
  reconnectRepositoryAuth: reconnectRepositoryAuthMock,
  removeRepositoryProject: removeRepositoryProjectMock,
  saveSecretCredential: saveSecretCredentialMock,
  updateRepositoryProject: updateRepositoryProjectMock,
  validateUnityExecutablePath: validateUnityExecutablePathMock,
}));

vi.mock("../services/auth", () => ({
  loadAuthProviders: loadAuthProvidersMock,
  loginWithGithubAuth: loginWithGithubAuthMock,
}));

afterEach(() => {
  vi.useRealTimers();
  cleanup();
  vi.clearAllMocks();
});

beforeEach(() => {
  connectRepositoryAuthMock.mockResolvedValue(undefined);
  detectRepositoryProviderMock.mockResolvedValue(buildRepositoryProvider());
  disconnectRepositoryAuthMock.mockResolvedValue(undefined);
  loadDefaultProjectWorkspaceRootMock.mockImplementation(
    async (projectName?: string | null) =>
      projectName?.trim()
        ? `C:/Users/indie/HGPWorkspaces/${projectName.trim()}`
        : "C:/Users/indie/HGPWorkspaces",
  );
  loadAuthProvidersMock.mockResolvedValue([buildGithubAuthProvider()]);
  loadRepositoryInspectionMock.mockResolvedValue({ repositories: [] });
  loadRepositoryProjectDetailMock.mockResolvedValue(buildRepositoryDetail());
  loadSecretSettingsMock.mockResolvedValue(buildSecretSettings());
  loadUnityAdapterSettingsMock.mockResolvedValue(buildUnityAdapterSettings());
  loginWithGithubAuthMock.mockResolvedValue(buildGithubAuthProvider());
  reconnectRepositoryAuthMock.mockResolvedValue(undefined);
  removeRepositoryProjectMock.mockResolvedValue(buildProjectRemovalReport());
  saveSecretCredentialMock.mockResolvedValue(303);
  updateRepositoryProjectMock.mockResolvedValue(undefined);
  validateUnityExecutablePathMock.mockResolvedValue(
    buildUnityExecutableValidation(),
  );
});

describe("RepositoryProjectDetail", () => {
  it("shows only the workspace root override in the paths tab", async () => {
    renderProjectDetail();

    fireEvent.click(await screen.findByRole("tab", { name: "Paths" }));

    const workspaceRootInput = await screen.findByDisplayValue(
      "C:/Users/indie/HGPWorkspaces/Revolutions",
    );

    expect(workspaceRootInput).toHaveAttribute(
      "title",
      "C:/Users/indie/HGPWorkspaces/Revolutions",
    );
    expect(
      screen.queryByText("Artifacts root override"),
    ).not.toBeInTheDocument();
  });

  it("debounces repository URL access checks and does not loop after the result arrives", async () => {
    renderProjectDetail();

    fireEvent.click(await screen.findByRole("tab", { name: "Repository" }));

    const repositoryUrlInput = await screen.findByRole("textbox", {
      name: "Repository URL",
    });

    vi.useFakeTimers();

    fireEvent.change(repositoryUrlInput, {
      target: { value: "https://github.com/indiegabo/rev" },
    });
    fireEvent.change(repositoryUrlInput, {
      target: { value: "https://github.com/indiegabo/revolutions" },
    });
    fireEvent.change(repositoryUrlInput, {
      target: { value: "https://github.com/indiegabo/revolutions-next.git" },
    });

    expect(detectRepositoryProviderMock).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(799);
    });

    expect(detectRepositoryProviderMock).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });

    expect(detectRepositoryProviderMock).toHaveBeenCalledTimes(1);
    expect(detectRepositoryProviderMock).toHaveBeenLastCalledWith(
      "https://github.com/indiegabo/revolutions-next.git",
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1600);
    });

    expect(detectRepositoryProviderMock).toHaveBeenCalledTimes(1);
  });

  it("renders wizard step tabs for repository projects", async () => {
    renderProjectDetail();

    expect(
      await screen.findByRole("heading", { name: "Revolutions" }),
    ).toBeInTheDocument();

    const tabLabels = screen
      .getAllByRole("tab")
      .map((tab) => tab.getAttribute("aria-label"));

    expect(tabLabels).toEqual([
      "Identity",
      "Repository",
      "Build Targets",
      "Publish Destinations",
      "Paths",
    ]);
    expect(screen.queryByRole("tab", { name: "Review" })).toBeNull();
    expect(screen.queryByRole("tab", { name: "Runtime Status" })).toBeNull();
    expect(screen.getByRole("tab", { name: "Identity" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("switches the access tab to workspace for local projects", async () => {
    loadRepositoryProjectDetailMock.mockResolvedValue(
      buildRepositoryDetail({
        auth_binding_status: "not_required",
        auth_requirement_status: "not_required",
        auth_status_message: "Local workspace projects do not need auth.",
        local_path: "C:/projects/revolutions-local",
        repo_url: "C:/projects/revolutions-local",
        source_mode: "local_workspace",
      }),
    );

    renderProjectDetail();

    expect(
      await screen.findByRole("tab", { name: "Workspace" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Workspace" }));

    expect(
      await screen.findByDisplayValue("C:/projects/revolutions-local"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Repository access" }),
    ).toBeNull();
  });

  it("opens the publish destination overlay flow from the publish tab", async () => {
    renderProjectDetail();

    fireEvent.click(
      await screen.findByRole("tab", { name: "Publish Destinations" }),
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "Add destination" }),
    );

    expect(
      screen.getByRole("menu", { name: "Destination list" }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Itch" }));

    expect(
      await screen.findByRole("dialog", { name: "Add Itch destination" }),
    ).toBeInTheDocument();
  });

  it("locks the rebuilt editor when repository processes are running", async () => {
    loadRepositoryProjectDetailMock.mockResolvedValue(
      buildRepositoryDetail({
        running_build_runs: 1,
        release_queue: [
          {
            build_process_active: true,
            engine_version: "6000.0.23f1",
            git_tag: "v1.1.12",
            planned: false,
            publish_process_active: false,
            queued_build_runs: 0,
            queued_publish_runs: 0,
            release_run_id: 7,
            running_build_runs: 1,
            running_publish_runs: 0,
            status: "running",
            terminal_build_runs: 0,
            terminal_publish_runs: 0,
            total_build_runs: 1,
            total_publish_runs: 0,
          },
        ],
      }),
    );

    renderProjectDetail();

    expect(
      await screen.findByText(
        "Project changes are available only when no related processes are running.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save Changes" })).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Remove Project" }),
    ).toBeDisabled();
    expect(screen.getByRole("tab", { name: "Build Targets" })).toBeDisabled();
  });

  it("does not render a project-level process priority control in the targets tab", async () => {
    renderProjectDetail();

    fireEvent.click(await screen.findByRole("tab", { name: "Build Targets" }));

    await waitFor(() => {
      expect(
        screen.queryByRole("combobox", {
          name: "Build process priority",
        }),
      ).not.toBeInTheDocument();
    });
  });
});

function renderProjectDetail(
  props: Partial<React.ComponentProps<typeof RepositoryProjectDetail>> = {},
) {
  return render(
    <OverlayProvider>
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        repositoryId={1}
        {...props}
      />
    </OverlayProvider>,
  );
}

function buildRepositoryDetail(
  overrides: Partial<Record<string, unknown>> = {},
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
        diagnostic_message: "Ready for host-native execution.",
        diagnostic_status: "ready",
        enabled: true,
        host_native_diagnostics: buildUnityExecutableValidation(),
        repository_id: 1,
        repository_name: "Revolutions",
        runner_type: "host-native",
        target_name: "Windows",
        unity_build_method: "Builder.PerformWindows",
        unity_target_platform: "StandaloneWindows64",
      },
    ],
    credentials: {
      config_message: "Stored GitHub login metadata is valid.",
      config_status: "ready",
      credential_id: 101,
      kind: "git-http-github-host-login",
      name: "GitHub.com",
    },
    default_branch: "main",
    enabled: true,
    enabled_build_target_count: 1,
    engine_kind: "unity",
    last_seen_tag: "v1.1.12",
    pending_release_count: 1,
    polling_interval_seconds: 30,
    publish_targets: [],
    queued_build_runs: 1,
    queued_publish_runs: 0,
    release_queue: [
      {
        build_process_active: false,
        engine_version: "6000.0.23f1",
        git_tag: "v1.1.12",
        planned: true,
        publish_process_active: false,
        queued_build_runs: 1,
        queued_publish_runs: 0,
        release_run_id: 7,
        running_build_runs: 0,
        running_publish_runs: 0,
        status: "queued",
        terminal_build_runs: 0,
        terminal_publish_runs: 0,
        total_build_runs: 2,
        total_publish_runs: 0,
      },
    ],
    local_path: null,
    repo_url: "https://github.com/indiegabo/revolutions.git",
    repository_id: 1,
    repository_name: "Revolutions",
    running_build_runs: 0,
    running_publish_runs: 0,
    source_mode: "managed_repository",
    source_instance_url: "https://github.com",
    source_provider_id: "github",
    visibility_status: "private",
    workspace_root_override: null,
    ...overrides,
  };
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
    normalized_url: "https://github.com/indiegabo/revolutions.git",
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
      {
        config_summary: {
          message: "Itch API key is present.",
          missing_required_keys: [],
          status: "ready",
          top_level_keys: ["api_key"],
        },
        created_at: "2026-05-19T00:00:00Z",
        credential_id: 202,
        kind: "itch-api-key",
        name: "Itch Release",
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

function buildUnityAdapterSettings() {
  return {
    capability_profile: {
      discovered_editors: [
        {
          executable_exists: true,
          executable_is_file: true,
          executable_path:
            "C:/Program Files/Unity/Hub/Editor/6000.4.3f1/Editor/Unity.exe",
          install_root_path:
            "C:/Program Files/Unity/Hub/Editor/6000.4.3f1/Editor",
          message: "Discovered Unity editor 6000.4.3f1.",
          source: "unity_hub_editor_root",
          status: "ready",
          supported_build_targets: ["windows"],
          version: "6000.4.3f1",
        },
      ],
    },
  };
}

function buildUnityExecutableValidation() {
  return {
    additional_argument_count: 0,
    environment_variable_count: 0,
    message: "Unity executable is ready.",
    process_priority: "low",
    runner_family: "host-native",
    status: "ready",
    unity_executable_exists: true,
    unity_executable_is_file: true,
    unity_executable_path:
      "C:/Program Files/Unity/Hub/Editor/6000.0.23f1/Editor/Unity.exe",
  };
}

function buildProjectRemovalReport(
  overrides: Partial<Record<string, unknown>> = {},
) {
  return {
    repository_id: 1,
    repository_name: "Revolutions",
    strategy: "detach",
    release_run_count: 1,
    build_run_count: 1,
    publish_run_count: 0,
    queue_message_count: 1,
    coordination_lease_count: 0,
    idempotency_key_count: 1,
    removed_paths: [],
    missing_paths: [],
    skipped_paths: [],
    ...overrides,
  };
}
