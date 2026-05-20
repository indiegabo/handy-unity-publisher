import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { RepositoryProjectDetail } from "./RepositoryProjectDetail";

const {
  connectRepositoryAuthMock,
  detectRepositoryProviderMock,
  disconnectRepositoryAuthMock,
  loadAuthProvidersMock,
  loadRepositoryProjectDetailMock,
  loadSecretSettingsMock,
  loginWithGithubAuthMock,
  reconnectRepositoryAuthMock,
  saveSecretCredentialMock,
  updateRepositoryProjectMock,
  validateUnityExecutablePathMock,
} = vi.hoisted(() => ({
  connectRepositoryAuthMock: vi.fn(),
  detectRepositoryProviderMock: vi.fn(),
  disconnectRepositoryAuthMock: vi.fn(),
  loadAuthProvidersMock: vi.fn(),
  loadRepositoryProjectDetailMock: vi.fn(),
  loadSecretSettingsMock: vi.fn(),
  loginWithGithubAuthMock: vi.fn(),
  reconnectRepositoryAuthMock: vi.fn(),
  saveSecretCredentialMock: vi.fn(),
  updateRepositoryProjectMock: vi.fn(),
  validateUnityExecutablePathMock: vi.fn(),
}));

vi.mock("../services/projects", () => ({
  connectRepositoryAuth: connectRepositoryAuthMock,
  detectRepositoryProvider: detectRepositoryProviderMock,
  disconnectRepositoryAuth: disconnectRepositoryAuthMock,
  loadRepositoryProjectDetail: loadRepositoryProjectDetailMock,
  loadSecretSettings: loadSecretSettingsMock,
  reconnectRepositoryAuth: reconnectRepositoryAuthMock,
  saveSecretCredential: saveSecretCredentialMock,
  updateRepositoryProject: updateRepositoryProjectMock,
  validateUnityExecutablePath: validateUnityExecutablePathMock,
}));

vi.mock("../services/auth", () => ({
  loadAuthProviders: loadAuthProvidersMock,
  loginWithGithubAuth: loginWithGithubAuthMock,
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

beforeEach(() => {
  connectRepositoryAuthMock.mockResolvedValue(undefined);
  detectRepositoryProviderMock.mockResolvedValue(buildRepositoryProvider());
  disconnectRepositoryAuthMock.mockResolvedValue(undefined);
  loadAuthProvidersMock.mockResolvedValue([buildGithubAuthProvider()]);
  loadRepositoryProjectDetailMock.mockResolvedValue(buildRepositoryDetail());
  loadSecretSettingsMock.mockResolvedValue(buildSecretSettings());
  loginWithGithubAuthMock.mockResolvedValue(buildGithubAuthProvider());
  reconnectRepositoryAuthMock.mockResolvedValue(undefined);
  saveSecretCredentialMock.mockResolvedValue(undefined);
  updateRepositoryProjectMock.mockResolvedValue(undefined);
  validateUnityExecutablePathMock.mockResolvedValue(
    buildUnityExecutableValidation(),
  );
});

describe("RepositoryProjectDetail", () => {
  it("renders icon tabs with tooltips for each project section", async () => {
    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        repositoryId={1}
      />,
    );

    expect(
      await screen.findByRole("heading", { name: "Revolutions" }),
    ).toBeInTheDocument();

    await waitFor(() => {
      expect(loadRepositoryProjectDetailMock).toHaveBeenCalledWith(1);
    });

    expect(
      await screen.findByRole("tablist", { name: "Project detail sections" }),
    ).toBeInTheDocument();
    const tabLabels = screen
      .getAllByRole("tab")
      .map((tab) => tab.getAttribute("aria-label"));

    expect(tabLabels).toEqual([
      "Project Settings",
      "Repository",
      "Paths",
      "Build Targets",
      "Publish Destinations",
      "Runtime Status",
    ]);
    expect(
      screen.getByRole("tab", { name: "Project Settings" }),
    ).toHaveAttribute("aria-selected", "true");
  });

  it("shows repository configuration in its own tab", async () => {
    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        repositoryId={1}
      />,
    );

    fireEvent.click(await screen.findByRole("tab", { name: "Repository" }));

    expect(await screen.findByLabelText("Repository URL")).toBeInTheDocument();
    expect(screen.getByText("Default branch")).toBeInTheDocument();
    expect(screen.getByText("Polling interval")).toBeInTheDocument();
    expect(screen.getByText("Repository access")).toBeInTheDocument();
  });

  it("shows repository path overrides in their own tab", async () => {
    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        repositoryId={1}
      />,
    );

    fireEvent.click(await screen.findByRole("tab", { name: "Paths" }));

    expect(
      await screen.findByText("Artifacts root override"),
    ).toBeInTheDocument();
    expect(screen.getByText("Workspace root override")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Paths" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("switches between project detail tabs", async () => {
    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        repositoryId={1}
      />,
    );

    fireEvent.click(await screen.findByRole("tab", { name: "Build Targets" }));

    expect(
      await screen.findByRole("button", { name: "Add target" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Build Targets" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("re-detects repository access only after the operator edits the repository url", async () => {
    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        repositoryId={1}
      />,
    );

    fireEvent.click(await screen.findByRole("tab", { name: "Repository" }));

    const repositoryUrlField = await screen.findByLabelText("Repository URL");
    fireEvent.change(repositoryUrlField, {
      target: {
        value: "https://github.com/indiegabo/revolutions-next.git",
      },
    });

    await waitFor(() => {
      expect(detectRepositoryProviderMock).toHaveBeenCalledWith(
        "https://github.com/indiegabo/revolutions-next.git",
      );
    });
  });

  it("forces a browser relogin when the repository is marked reauth required", async () => {
    loadRepositoryProjectDetailMock.mockResolvedValue(
      buildRepositoryDetail({
        auth_binding_status: "reauth_required",
        auth_status_message: "GitHub access must be refreshed.",
      }),
    );

    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        repositoryId={1}
      />,
    );

    fireEvent.click(await screen.findByRole("tab", { name: "Repository" }));

    fireEvent.click(
      await screen.findByRole("button", { name: "Reconnect GitHub login" }),
    );

    await waitFor(() => {
      expect(loginWithGithubAuthMock).toHaveBeenCalledWith({ force: true });
    });
  });

  it("does not expose a custom repository credential form", async () => {
    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        repositoryId={1}
      />,
    );

    expect(
      screen.queryByRole("button", { name: "New credential" }),
    ).not.toBeInTheDocument();
  });

  it("shows publish destination content when the destinations tab is selected", async () => {
    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        repositoryId={1}
      />,
    );

    fireEvent.click(
      await screen.findByRole("tab", {
        name: "Publish Destinations",
      }),
    );

    expect(await screen.findByText("Draft impact")).toBeInTheDocument();
  });

  it("persists edited project fields and returns to a saved draft state after reload", async () => {
    loadRepositoryProjectDetailMock.mockReset();
    loadRepositoryProjectDetailMock
      .mockResolvedValueOnce(buildRepositoryDetail())
      .mockResolvedValueOnce(
        buildRepositoryDetail({
          repository_name: "Revolutions Redux",
        }),
      );

    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        repositoryId={1}
      />,
    );

    const projectNameField = await screen.findByLabelText("Project name");
    const saveButton = screen.getByRole("button", { name: "Save Changes" });

    expect(saveButton).toBeDisabled();
    expect(screen.getByText("Saved")).toBeInTheDocument();

    fireEvent.change(projectNameField, {
      target: { value: "Revolutions Redux" },
    });

    await waitFor(() => {
      expect(saveButton).toBeEnabled();
    });
    expect(screen.getByText("Unsaved changes")).toBeInTheDocument();

    fireEvent.click(saveButton);

    await waitFor(() => {
      expect(updateRepositoryProjectMock).toHaveBeenCalledWith(
        expect.objectContaining({
          repository_id: 1,
          name: "Revolutions Redux",
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByRole("textbox", { name: "Project name" })).toHaveValue(
        "Revolutions Redux",
      );
      expect(
        screen.getByRole("button", { name: "Save Changes" }),
      ).toBeDisabled();
    });

    expect(
      screen.getByText("Saved changes for Revolutions Redux."),
    ).toBeInTheDocument();
    expect(screen.getByText("Saved")).toBeInTheDocument();
  });

  it("reloads the persisted project snapshot and discards unsaved edits", async () => {
    loadRepositoryProjectDetailMock.mockReset();
    loadRepositoryProjectDetailMock
      .mockResolvedValueOnce(buildRepositoryDetail())
      .mockResolvedValueOnce(buildRepositoryDetail());

    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        repositoryId={1}
      />,
    );

    const projectNameField = await screen.findByLabelText("Project name");

    fireEvent.change(projectNameField, {
      target: { value: "Revolutions Draft" },
    });

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Save Changes" }),
      ).toBeEnabled();
    });

    fireEvent.click(screen.getByRole("button", { name: "Reload" }));

    await waitFor(() => {
      expect(screen.getByRole("textbox", { name: "Project name" })).toHaveValue(
        "Revolutions",
      );
      expect(
        screen.getByRole("button", { name: "Save Changes" }),
      ).toBeDisabled();
    });

    expect(screen.getByText("Saved")).toBeInTheDocument();
    expect(updateRepositoryProjectMock).not.toHaveBeenCalled();
  });

  it("locks project editing while related processes are running", async () => {
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

    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        repositoryId={1}
      />,
    );

    expect(
      await screen.findByText(
        "Project changes are available only when no related processes are running.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save Changes" })).toBeDisabled();

    const buildTargetsTab = screen.getByRole("tab", { name: "Build Targets" });
    expect(buildTargetsTab).toBeDisabled();

    fireEvent.click(buildTargetsTab);

    expect(
      screen.getByRole("tab", { name: "Project Settings" }),
    ).toHaveAttribute("aria-selected", "true");
    expect(
      screen.queryByRole("button", { name: "Add target" }),
    ).not.toBeInTheDocument();
  });
});

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
    repo_url: "https://github.com/indiegabo/revolutions.git",
    repository_id: 1,
    repository_name: "Revolutions",
    running_build_runs: 0,
    running_publish_runs: 0,
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
