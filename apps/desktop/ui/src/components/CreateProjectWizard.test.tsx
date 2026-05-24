import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  CreateProjectWizard,
  type CreateProjectWizardSnapshot,
} from "./CreateProjectWizard";

const {
  createRepositoryProjectMock,
  detectRepositoryProviderMock,
  loadRepositoryInspectionMock,
  loadSecretSettingsMock,
  loadUnityAdapterSettingsMock,
  saveSecretCredentialMock,
  validateUnityExecutablePathMock,
  loadAuthProvidersMock,
  loginWithGithubAuthMock,
} = vi.hoisted(() => ({
  createRepositoryProjectMock: vi.fn(),
  detectRepositoryProviderMock: vi.fn(),
  loadRepositoryInspectionMock: vi.fn(),
  loadSecretSettingsMock: vi.fn(),
  loadUnityAdapterSettingsMock: vi.fn(),
  saveSecretCredentialMock: vi.fn(),
  validateUnityExecutablePathMock: vi.fn(),
  loadAuthProvidersMock: vi.fn(),
  loginWithGithubAuthMock: vi.fn(),
}));

vi.mock("../services/projects", () => ({
  createRepositoryProject: createRepositoryProjectMock,
  detectRepositoryProvider: detectRepositoryProviderMock,
  loadRepositoryInspection: loadRepositoryInspectionMock,
  loadSecretSettings: loadSecretSettingsMock,
  loadUnityAdapterSettings: loadUnityAdapterSettingsMock,
  saveSecretCredential: saveSecretCredentialMock,
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
  createRepositoryProjectMock.mockResolvedValue({ repository_id: 1 });
  detectRepositoryProviderMock.mockResolvedValue(buildRepositoryProvider());
  loadRepositoryInspectionMock.mockResolvedValue({ repositories: [] });
  loadSecretSettingsMock.mockResolvedValue(buildSecretSettings());
  loadUnityAdapterSettingsMock.mockResolvedValue(buildUnityAdapterSettings());
  saveSecretCredentialMock.mockResolvedValue(303);
  validateUnityExecutablePathMock.mockResolvedValue(
    buildUnityExecutableValidation(),
  );
  loadAuthProvidersMock.mockResolvedValue([buildGithubAuthProvider()]);
  loginWithGithubAuthMock.mockResolvedValue(buildGithubAuthProvider());
});

describe("CreateProjectWizard", () => {
  it("shows the active step inside a compact status header", async () => {
    render(<CreateProjectWizard onCreated={vi.fn()} onManageAuth={vi.fn()} />);

    expect(await screen.findByText("1. Identity")).toBeInTheDocument();
    expect(screen.getByText("1 of 6")).toBeInTheDocument();

    fireEvent.change(await screen.findByLabelText("Project name"), {
      target: { value: "Red Horizon" },
    });

    fireEvent.click(screen.getByRole("button", { name: "Next" }));

    expect(await screen.findByText("2. Repository")).toBeInTheDocument();
    expect(screen.getByText("2 of 6")).toBeInTheDocument();
  });

  it("renders access guidance inside a dedicated support panel", async () => {
    render(<CreateProjectWizard onCreated={vi.fn()} onManageAuth={vi.fn()} />);

    expect(
      screen.queryByRole("heading", { name: "Project adapters" }),
    ).not.toBeInTheDocument();

    fireEvent.change(await screen.findByLabelText("Project name"), {
      target: { value: "Red Horizon" },
    });

    fireEvent.click(screen.getByRole("button", { name: "Next" }));

    expect(
      await screen.findByRole("heading", { name: "Repository access" }),
    ).toBeInTheDocument();
    expect(screen.getByText("2. Repository")).toBeInTheDocument();
    expect(
      screen.queryByRole("textbox", { name: "Default branch" }),
    ).not.toBeInTheDocument();
  });

  it("shows local workspace fields instead of repository access fields for local workspace access", async () => {
    render(
      <CreateProjectWizard
        initialSnapshot={buildLocalAccessStepSnapshot()}
        onCreated={vi.fn()}
        onManageAuth={vi.fn()}
      />,
    );

    expect(await screen.findByText("2. Workspace")).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Local workspace source" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Local workspace path")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Choose workspace" }),
    ).toBeInTheDocument();
    expect(
      screen.getByDisplayValue("C:/projects/red-horizon"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("textbox", { name: "Repository URL" }),
    ).not.toBeInTheDocument();
  });

  it("shows an unavailable state instead of Unity fields for unsupported engines", async () => {
    render(
      <CreateProjectWizard
        initialSnapshot={buildGodotTargetsStepSnapshot()}
        onCreated={vi.fn()}
        onManageAuth={vi.fn()}
      />,
    );

    expect(await screen.findByText("3. Build Targets")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Godot build target setup does not have a create-project adapter yet.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Godot target adapter" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByLabelText("Unity target platform"),
    ).not.toBeInTheDocument();
  });

  it("lists detected Unity editors and fills the executable path from the selected install", async () => {
    render(
      <CreateProjectWizard
        initialSnapshot={buildUnityTargetsStepSnapshot()}
        onCreated={vi.fn()}
        onManageAuth={vi.fn()}
      />,
    );

    const installedEditorsSelect = await screen.findByRole("combobox", {
      name: "Installed Unity editors",
    });

    expect(installedEditorsSelect).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Choose Unity executable" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("option", { name: "6000.1.9f1" }),
    ).not.toBeInTheDocument();
    await waitFor(() => {
      expect(installedEditorsSelect).toHaveTextContent("6000.3.11f1");
      expect(installedEditorsSelect).toHaveTextContent("6000.4.3f1");
    });

    fireEvent.change(installedEditorsSelect, {
      target: {
        value: "C:/Program Files/Unity/Hub/Editor/6000.4.3f1/Editor/Unity.exe",
      },
    });

    await waitFor(() => {
      expect(
        screen.getByDisplayValue(
          "C:/Program Files/Unity/Hub/Editor/6000.4.3f1/Editor/Unity.exe",
        ),
      ).toBeInTheDocument();
      expect(validateUnityExecutablePathMock).toHaveBeenCalledWith(
        "C:/Program Files/Unity/Hub/Editor/6000.4.3f1/Editor/Unity.exe",
      );
    });
  });

  it("delegates explicit cancel requests through the wizard footer", () => {
    const onRequestClose = vi.fn();

    render(
      <CreateProjectWizard
        onCreated={vi.fn()}
        onManageAuth={vi.fn()}
        onRequestClose={onRequestClose}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(onRequestClose).toHaveBeenCalledTimes(1);
  });

  it("resumes the provided snapshot at the saved access step", async () => {
    render(
      <CreateProjectWizard
        initialSnapshot={buildAccessStepSnapshot()}
        onCreated={vi.fn()}
        onManageAuth={vi.fn()}
      />,
    );

    expect(
      await screen.findByRole("heading", { name: "Repository access" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Repository URL" })).toHaveValue(
      "https://github.com/indiegabo/red-horizon.git",
    );
    expect(
      screen.getByRole("combobox", { name: "Repository visibility" }),
    ).toHaveValue("public");
    expect(screen.queryByLabelText("Project name")).not.toBeInTheDocument();
  });

  it("shows a retryable inventory failure and blocks advancement until it recovers", async () => {
    loadRepositoryInspectionMock
      .mockRejectedValueOnce(new Error("Inventory offline"))
      .mockResolvedValue({ repositories: [] });

    render(<CreateProjectWizard onCreated={vi.fn()} onManageAuth={vi.fn()} />);

    expect(await screen.findByText("Inventory offline")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Retry inventory load" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Next" })).toBeDisabled();

    fireEvent.click(
      screen.getByRole("button", { name: "Retry inventory load" }),
    );

    await waitFor(() => {
      expect(loadRepositoryInspectionMock).toHaveBeenCalledTimes(2);
      expect(screen.getByRole("button", { name: "Next" })).toBeEnabled();
    });
  });

  it("exposes retry actions for access-step support loads when they fail", async () => {
    loadAuthProvidersMock
      .mockRejectedValueOnce(new Error("Accounts offline"))
      .mockResolvedValue([buildGithubAuthProvider()]);
    loadSecretSettingsMock
      .mockRejectedValueOnce(new Error("Credentials offline"))
      .mockResolvedValue(buildSecretSettings());

    render(<CreateProjectWizard onCreated={vi.fn()} onManageAuth={vi.fn()} />);

    fireEvent.change(await screen.findByLabelText("Project name"), {
      target: { value: "Red Horizon" },
    });

    fireEvent.click(screen.getByRole("button", { name: "Next" }));

    expect(await screen.findByText("Repository URL")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Retry accounts" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Retry credentials" }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry accounts" }));
    fireEvent.click(screen.getByRole("button", { name: "Retry credentials" }));

    await waitFor(() => {
      expect(loadAuthProvidersMock).toHaveBeenCalledTimes(2);
      expect(loadSecretSettingsMock).toHaveBeenCalledTimes(2);
      expect(
        screen.queryByRole("button", { name: "Retry accounts" }),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: "Retry credentials" }),
      ).not.toBeInTheDocument();
    });
  });

  it("wires the GitHub login result back into the access step", async () => {
    loadAuthProvidersMock.mockResolvedValueOnce([
      {
        ...buildGithubAuthProvider(),
        credential_id: null,
        status: "disconnected",
        status_message: "GitHub login is ready to connect.",
      },
    ]);

    render(<CreateProjectWizard onCreated={vi.fn()} onManageAuth={vi.fn()} />);

    fireEvent.change(await screen.findByLabelText("Project name"), {
      target: { value: "Red Horizon" },
    });

    fireEvent.click(screen.getByRole("button", { name: "Next" }));

    fireEvent.change(
      await screen.findByRole("textbox", { name: "Repository URL" }),
      {
        target: { value: "https://github.com/indiegabo/red-horizon.git" },
      },
    );
    fireEvent.change(
      screen.getByRole("combobox", {
        name: "Repository visibility",
      }),
      {
        target: { value: "private" },
      },
    );

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Log in and connect" }),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "Open accounts" }),
      ).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Log in and connect" }));

    await waitFor(() => {
      expect(loginWithGithubAuthMock).toHaveBeenCalledTimes(1);
      expect(
        screen.getByText(
          "GitHub login connected for this project. Creating the project will save the connection.",
        ),
      ).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Next" })).toBeEnabled();
    });
  });

  it("creates the project directly from the review step", async () => {
    const onCreated = vi.fn();

    render(
      <CreateProjectWizard
        initialSnapshot={buildReviewSnapshot()}
        onCreated={onCreated}
        onManageAuth={vi.fn()}
      />,
    );

    const createButton = await screen.findByRole("button", {
      name: "Create project",
    });

    await waitFor(() => {
      expect(screen.getAllByText("Public").length).toBeGreaterThan(0);
    });

    expect(createButton).toBeEnabled();

    fireEvent.click(createButton);

    await waitFor(() => {
      expect(createRepositoryProjectMock).toHaveBeenCalledTimes(1);
      expect(onCreated).toHaveBeenCalledWith(1);
    });
  });

  it("creates a local workspace project from the review step", async () => {
    const onCreated = vi.fn();

    render(
      <CreateProjectWizard
        initialSnapshot={buildLocalReviewSnapshot()}
        onCreated={onCreated}
        onManageAuth={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText("6. Review")).toBeInTheDocument();
    });

    const createButton = await screen.findByRole("button", {
      name: "Create project",
    });
    expect(createButton).toBeEnabled();

    fireEvent.click(createButton);

    await waitFor(() => {
      expect(createRepositoryProjectMock).toHaveBeenCalledTimes(1);
      const payload = createRepositoryProjectMock.mock.calls[0]?.[0];
      expect(payload).toEqual(
        expect.objectContaining({
          source_mode: "local_workspace",
          local_path: "C:/projects/red-horizon",
        }),
      );
      expect(payload?.repository_url ?? null).toBeNull();
      expect(payload?.repository_access_assessment ?? null).toBeNull();
      expect(payload?.repository_credentials_id ?? null).toBeNull();
      expect(onCreated).toHaveBeenCalledWith(1);
    });
  });

  it("keeps late-step path validation local instead of advancing into review", async () => {
    render(
      <CreateProjectWizard
        initialSnapshot={buildInvalidPathStepSnapshot()}
        onCreated={vi.fn()}
        onManageAuth={vi.fn()}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Next" }));

    await waitFor(() => {
      expect(screen.getByText("5. Paths")).toBeInTheDocument();
      expect(
        screen.getByText("Artifacts root override must be an absolute path."),
      ).toBeInTheDocument();
    });

    expect(createRepositoryProjectMock).not.toHaveBeenCalled();
    expect(
      screen.queryByLabelText(
        "I reviewed the repository access, targets, publish destinations, and path overrides for this project.",
      ),
    ).not.toBeInTheDocument();
  });
});

function buildReviewSnapshot(): CreateProjectWizardSnapshot {
  return {
    attemptedSteps: {
      access: false,
      identity: false,
      paths: false,
      publish: false,
      review: false,
      targets: false,
    },
    currentStepIndex: 5,
    draft: {
      artifactsRootOverride: "",
      buildTargets: [
        {
          buildMethod: "Builder.PerformWindows",
          id: "target-1",
          name: "Windows",
          targetPlatform: "StandaloneWindows64",
          unityExecutablePath:
            "C:/Program Files/Unity/Hub/Editor/6000.0.23f1/Editor/Unity.exe",
        },
      ],
      engineKind: "unity",
      localPath: "",
      name: "Red Horizon",
      pollingIntervalSeconds: "300",
      projectKind: "repository",
      publishDestinations: [],
      repositoryUrl: "https://github.com/indiegabo/red-horizon.git",
      repositoryVisibility: "public",
      workspaceRootOverride: "",
    },
    expandedTargetIds: {
      "target-1": true,
    },
    pathDiagnostics: {
      "target-1": buildUnityExecutableValidation(),
    },
    pendingBuildTargetRemovalId: null,
    repositoryCredentialId: null,
    touchedFields: {},
  };
}

function buildInvalidPathStepSnapshot(): CreateProjectWizardSnapshot {
  const snapshot = buildReviewSnapshot();

  return {
    ...snapshot,
    currentStepIndex: 4,
    draft: {
      ...snapshot.draft,
      artifactsRootOverride: "relative/artifacts",
    },
  };
}

function buildAccessStepSnapshot(): CreateProjectWizardSnapshot {
  return {
    ...buildReviewSnapshot(),
    currentStepIndex: 1,
  };
}

function buildLocalAccessStepSnapshot(): CreateProjectWizardSnapshot {
  const snapshot = buildReviewSnapshot();

  return {
    ...snapshot,
    currentStepIndex: 1,
    draft: {
      ...snapshot.draft,
      engineKind: "unity",
      localPath: "C:/projects/red-horizon",
      projectKind: "local",
      repositoryUrl: "",
    },
  };
}

function buildLocalReviewSnapshot(): CreateProjectWizardSnapshot {
  const snapshot = buildReviewSnapshot();

  return {
    ...snapshot,
    draft: {
      ...snapshot.draft,
      localPath: "C:/projects/red-horizon",
      projectKind: "local",
      repositoryUrl: "",
    },
  };
}

function buildGodotTargetsStepSnapshot(): CreateProjectWizardSnapshot {
  const snapshot = buildReviewSnapshot();

  return {
    ...snapshot,
    currentStepIndex: 2,
    draft: {
      ...snapshot.draft,
      engineKind: "godot",
    },
  };
}

function buildUnityTargetsStepSnapshot(): CreateProjectWizardSnapshot {
  return {
    ...buildReviewSnapshot(),
    currentStepIndex: 2,
    pathDiagnostics: {
      "target-1": null,
    },
  };
}

function buildUnityAdapterSettings() {
  return {
    capability_profile: {
      discovered_editors: [
        {
          executable_exists: false,
          executable_is_file: false,
          executable_path:
            "C:/Program Files/Unity/Hub/Editor/6000.1.9f1/Editor/Unity.exe",
          install_root_path:
            "C:/Program Files/Unity/Hub/Editor/6000.1.9f1/Editor",
          message:
            "Unity editor 6000.1.9f1 was found under C:/Program Files/Unity/Hub/Editor/6000.1.9f1/Editor but the expected executable path C:/Program Files/Unity/Hub/Editor/6000.1.9f1/Editor/Unity.exe is not a regular file.",
          source: "unity_hub_editor_root",
          status: "error_missing_executable",
          supported_build_targets: [],
          version: "6000.1.9f1",
        },
        {
          executable_exists: true,
          executable_is_file: true,
          executable_path:
            "C:/Program Files/Unity/Hub/Editor/6000.3.11f1/Editor/Unity.exe",
          install_root_path:
            "C:/Program Files/Unity/Hub/Editor/6000.3.11f1/Editor",
          message:
            "Discovered Unity editor 6000.3.11f1 via unity_hub_editor_root at C:/Program Files/Unity/Hub/Editor/6000.3.11f1/Editor/Unity.exe.",
          source: "unity_hub_editor_root",
          status: "ready",
          supported_build_targets: ["windows"],
          version: "6000.3.11f1",
        },
        {
          executable_exists: true,
          executable_is_file: true,
          executable_path:
            "C:/Program Files/Unity/Hub/Editor/6000.4.3f1/Editor/Unity.exe",
          install_root_path:
            "C:/Program Files/Unity/Hub/Editor/6000.4.3f1/Editor",
          message:
            "Discovered Unity editor 6000.4.3f1 via unity_hub_editor_root at C:/Program Files/Unity/Hub/Editor/6000.4.3f1/Editor/Unity.exe.",
          source: "unity_hub_editor_root",
          status: "ready",
          supported_build_targets: ["windows"],
          version: "6000.4.3f1",
        },
      ],
    },
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
