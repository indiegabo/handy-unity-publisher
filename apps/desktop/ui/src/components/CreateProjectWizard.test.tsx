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
  saveSecretCredentialMock,
  validateUnityExecutablePathMock,
  loadAuthProvidersMock,
  loginWithGithubAuthMock,
} = vi.hoisted(() => ({
  createRepositoryProjectMock: vi.fn(),
  detectRepositoryProviderMock: vi.fn(),
  loadRepositoryInspectionMock: vi.fn(),
  loadSecretSettingsMock: vi.fn(),
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
  saveSecretCredentialMock.mockResolvedValue(undefined);
  validateUnityExecutablePathMock.mockResolvedValue(
    buildUnityExecutableValidation(),
  );
  loadAuthProvidersMock.mockResolvedValue([buildGithubAuthProvider()]);
  loginWithGithubAuthMock.mockResolvedValue(buildGithubAuthProvider());
});

describe("CreateProjectWizard", () => {
  it("renders access guidance inside a dedicated support panel", async () => {
    render(<CreateProjectWizard onCreated={vi.fn()} onManageAuth={vi.fn()} />);

    expect(
      await screen.findByRole("heading", { name: "Repository projects" }),
    ).toBeInTheDocument();

    fireEvent.change(await screen.findByLabelText("Project name"), {
      target: { value: "Red Horizon" },
    });

    fireEvent.click(screen.getByRole("button", { name: "Next" }));

    expect(
      await screen.findByRole("heading", { name: "Repository access" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Repository" }),
    ).toBeInTheDocument();
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
    expect(
      screen.getByRole("textbox", { name: "Repository URL" }),
    ).toHaveValue("https://github.com/indiegabo/red-horizon.git");
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

  it("requires explicit review confirmation before creating the project", async () => {
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

    expect(createButton).toBeDisabled();

    fireEvent.click(
      screen.getByLabelText(
        "I reviewed the repository access, targets, publish destinations, and path overrides for this project.",
      ),
    );

    expect(createButton).toBeEnabled();

    fireEvent.click(createButton);

    await waitFor(() => {
      expect(createRepositoryProjectMock).toHaveBeenCalledTimes(1);
      expect(onCreated).toHaveBeenCalledWith(1);
    });
  });

  it("requires a fresh review confirmation after leaving the review step", async () => {
    render(
      <CreateProjectWizard
        initialSnapshot={buildConfirmedReviewSnapshot()}
        onCreated={vi.fn()}
        onManageAuth={vi.fn()}
      />,
    );

    const createButton = await screen.findByRole("button", {
      name: "Create project",
    });

    expect(createButton).toBeEnabled();
    expect(screen.getByText("Ready to create")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Previous" }));

    expect(await screen.findByRole("heading", { name: "Paths" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Next" }));

    expect(
      await screen.findByRole("heading", { name: "Final confirmation" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create project" })).toBeDisabled();
    expect(screen.getByText("Pending confirmation")).toBeInTheDocument();
    expect(
      screen.getByLabelText(
        "I reviewed the repository access, targets, publish destinations, and path overrides for this project.",
      ),
    ).not.toBeChecked();
  });

  it("keeps late-step path validation local instead of advancing into review", async () => {
    render(
      <CreateProjectWizard
        initialSnapshot={buildInvalidPathStepSnapshot()}
        onCreated={vi.fn()}
        onManageAuth={vi.fn()}
      />,
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "Next" }),
    );

    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "Paths" }),
      ).toBeInTheDocument();
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
      defaultBranch: "main",
      engineKind: "unity",
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
    reviewConfirmed: false,
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
    reviewConfirmed: false,
  };
}

function buildAccessStepSnapshot(): CreateProjectWizardSnapshot {
  return {
    ...buildReviewSnapshot(),
    currentStepIndex: 1,
    reviewConfirmed: false,
  };
}

function buildConfirmedReviewSnapshot(): CreateProjectWizardSnapshot {
  return {
    ...buildReviewSnapshot(),
    reviewConfirmed: true,
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
