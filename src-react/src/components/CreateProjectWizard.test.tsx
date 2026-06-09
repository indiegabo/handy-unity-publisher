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

import {
  CreateProjectWizard,
  type CreateProjectWizardSnapshot,
} from "./CreateProjectWizard";
import OverlayProvider from "./OverlayManager";

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
  vi.useRealTimers();
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

  it("renders repository access controls in the repository access step", async () => {
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

  it("debounces repository access detection until typing settles", async () => {
    render(<CreateProjectWizard onCreated={vi.fn()} onManageAuth={vi.fn()} />);

    fireEvent.change(await screen.findByLabelText("Project name"), {
      target: { value: "Red Horizon" },
    });

    fireEvent.click(screen.getByRole("button", { name: "Next" }));

    const repositoryUrlInput = await screen.findByRole("textbox", {
      name: "Repository URL",
    });

    vi.useFakeTimers();

    fireEvent.change(repositoryUrlInput, {
      target: { value: "https://github.com/indiegabo/red" },
    });
    fireEvent.change(repositoryUrlInput, {
      target: { value: "https://github.com/indiegabo/red-horizon" },
    });
    fireEvent.change(repositoryUrlInput, {
      target: { value: "https://github.com/indiegabo/red-horizon.git" },
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
      "https://github.com/indiegabo/red-horizon.git",
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });

    expect(detectRepositoryProviderMock).toHaveBeenCalledTimes(1);
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

  it("adds a build target through the overlay and returns to a summary card", async () => {
    render(
      <OverlayProvider>
        <CreateProjectWizard
          initialSnapshot={buildEmptyUnityTargetsStepSnapshot()}
          onCreated={vi.fn()}
          onManageAuth={vi.fn()}
        />
      </OverlayProvider>,
    );

    expect(
      await screen.findByText("No build targets configured."),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Add target" }));

    const dialog = await screen.findByRole("dialog", {
      name: "Add build target",
    });

    fireEvent.change(within(dialog).getByLabelText("Unity target platform"), {
      target: { value: "StandaloneWindows64" },
    });
    expect(
      within(dialog).getByText("Default target name: Windows 64-bit"),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByText(
        "Default build method: HGPBuilder.PerformWindows64",
      ),
    ).toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole("button", { name: "Confirm" }));

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "Add build target" }),
      ).not.toBeInTheDocument();
    });

    expect(
      await screen.findByRole("heading", { name: "Windows 64-bit" }),
    ).toBeInTheDocument();
    expect(
      screen.getAllByText(/HGPBuilder\.PerformWindows64/).length,
    ).toBeGreaterThan(0);
  });

  it("lists the full supported Unity target catalog in the overlay", async () => {
    render(
      <OverlayProvider>
        <CreateProjectWizard
          initialSnapshot={buildEmptyUnityTargetsStepSnapshot()}
          onCreated={vi.fn()}
          onManageAuth={vi.fn()}
        />
      </OverlayProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Add target" }));

    const dialog = await screen.findByRole("dialog", {
      name: "Add build target",
    });

    const targetPlatformField = within(dialog).getByLabelText(
      "Unity target platform",
    ) as HTMLSelectElement;

    expect(
      Array.from(targetPlatformField.querySelectorAll("optgroup")).map(
        (group) => ({
          label: group.label,
          options: Array.from(group.querySelectorAll("option")).map(
            (option) => option.text,
          ),
        }),
      ),
    ).toEqual([
      {
        label: "Desktop",
        options: ["Windows 32-bit", "Windows 64-bit", "macOS", "Linux 64-bit"],
      },
      {
        label: "Mobile and XR",
        options: ["iOS", "Android", "tvOS", "visionOS"],
      },
      {
        label: "Web and Store",
        options: ["WebGL", "UWP"],
      },
      {
        label: "Consoles",
        options: [
          "PS4",
          "PS5",
          "Xbox One",
          "GameCore Xbox One",
          "GameCore Xbox Series",
          "Nintendo Switch",
        ],
      },
      {
        label: "Servers",
        options: ["Dedicated Server Linux"],
      },
    ]);

    expect(
      Array.from(targetPlatformField.options, (option) => option.text),
    ).toEqual([
      "Select a Unity target",
      "Windows 32-bit",
      "Windows 64-bit",
      "macOS",
      "Linux 64-bit",
      "iOS",
      "Android",
      "tvOS",
      "visionOS",
      "WebGL",
      "UWP",
      "PS4",
      "PS5",
      "Xbox One",
      "GameCore Xbox One",
      "GameCore Xbox Series",
      "Nintendo Switch",
      "Dedicated Server Linux",
    ]);

    fireEvent.change(targetPlatformField, {
      target: { value: "PS5" },
    });

    expect(
      within(dialog).getByText("Default target name: PS5"),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByText("Default build method: HGPBuilder.PerformPS5"),
    ).toBeInTheDocument();
  });

  it("blocks Unity target platforms that are already configured", async () => {
    render(
      <OverlayProvider>
        <CreateProjectWizard
          initialSnapshot={buildUnityTargetsStepSnapshot()}
          onCreated={vi.fn()}
          onManageAuth={vi.fn()}
        />
      </OverlayProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Add target" }));

    const dialog = await screen.findByRole("dialog", {
      name: "Add build target",
    });
    const targetPlatformField = within(dialog).getByLabelText(
      "Unity target platform",
    ) as HTMLSelectElement;
    const windowsOption = targetPlatformField.querySelector(
      'option[value="StandaloneWindows64"]',
    ) as HTMLOptionElement | null;

    expect(windowsOption?.disabled).toBe(true);

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Custom configuration" }),
    );

    expect(
      within(dialog).getByLabelText("Custom build method"),
    ).toHaveAttribute("placeholder", "HGPBuilder.PerformWindows32");
  });

  it("keeps the current target available while editing and blocks the others", async () => {
    render(
      <OverlayProvider>
        <CreateProjectWizard
          initialSnapshot={buildTargetsStepSnapshotWithMultipleTargets()}
          onCreated={vi.fn()}
          onManageAuth={vi.fn()}
        />
      </OverlayProvider>,
    );

    fireEvent.click(
      (await screen.findAllByRole("button", { name: "Edit" }))[1],
    );

    const dialog = await screen.findByRole("dialog", {
      name: "Edit build target",
    });
    const targetPlatformField = within(dialog).getByLabelText(
      "Unity target platform",
    ) as HTMLSelectElement;
    const macOsOption = targetPlatformField.querySelector(
      'option[value="StandaloneOSX"]',
    ) as HTMLOptionElement | null;
    const windowsOption = targetPlatformField.querySelector(
      'option[value="StandaloneWindows64"]',
    ) as HTMLOptionElement | null;

    expect(macOsOption?.disabled).toBe(false);
    expect(windowsOption?.disabled).toBe(true);
  });

  it("allows custom configuration for target name and method", async () => {
    render(
      <OverlayProvider>
        <CreateProjectWizard
          initialSnapshot={buildEmptyUnityTargetsStepSnapshot()}
          onCreated={vi.fn()}
          onManageAuth={vi.fn()}
        />
      </OverlayProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Add target" }));

    const dialog = await screen.findByRole("dialog", {
      name: "Add build target",
    });

    fireEvent.change(within(dialog).getByLabelText("Unity target platform"), {
      target: { value: "StandaloneLinux64" },
    });

    expect(
      within(dialog).getByText(
        "Default build method: HGPBuilder.PerformLinux64",
      ),
    ).toBeInTheDocument();

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Custom configuration" }),
    );

    expect(
      within(dialog).queryByLabelText("Unity target platform"),
    ).not.toBeInTheDocument();

    fireEvent.change(within(dialog).getByLabelText("Custom target name"), {
      target: { value: "Linux Release" },
    });

    fireEvent.change(within(dialog).getByLabelText("Custom build method"), {
      target: { value: "CustomBuilder.PerformLinuxRelease" },
    });

    fireEvent.click(within(dialog).getByRole("button", { name: "Confirm" }));

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "Add build target" }),
      ).not.toBeInTheDocument();
    });

    expect(
      screen.getAllByText(/CustomBuilder\.PerformLinuxRelease/).length,
    ).toBeGreaterThan(0);
    expect(
      screen.getByRole("heading", { name: "Linux Release" }),
    ).toBeInTheDocument();
  });

  it("lists detected Unity editors in the step and fills the shared executable path", async () => {
    render(
      <OverlayProvider>
        <CreateProjectWizard
          initialSnapshot={buildUnityTargetsStepSnapshot()}
          onCreated={vi.fn()}
          onManageAuth={vi.fn()}
        />
      </OverlayProvider>,
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
      expect(payload?.build_targets?.[0]?.process_priority).toBe("low");
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
        screen.getByText("Workspace root override must be an absolute path."),
      ).toBeInTheDocument();
    });

    expect(createRepositoryProjectMock).not.toHaveBeenCalled();
    expect(
      screen.queryByLabelText(
        "I reviewed the repository access, targets, publish destinations, and path overrides for this project.",
      ),
    ).not.toBeInTheDocument();
  });

  it("opens the publish destination overlay flow from the publish step", async () => {
    render(
      <OverlayProvider>
        <CreateProjectWizard
          initialSnapshot={buildPublishStepSnapshot()}
          onCreated={vi.fn()}
          onManageAuth={vi.fn()}
        />
      </OverlayProvider>,
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
      buildTargets: [
        {
          buildMethod: "HGPBuilder.PerformWindows64",
          id: "target-1",
          name: "Windows",
          targetPlatform: "StandaloneWindows64",
        },
      ],
      engineKind: "unity",
      localPath: "",
      name: "Red Horizon",
      pollingIntervalSeconds: "300",
      projectKind: "repository",
      processPriority: "low",
      publishDestinations: [],
      repositoryUrl: "https://github.com/indiegabo/red-horizon.git",
      repositoryVisibility: "public",
      unityExecutablePath:
        "C:/Program Files/Unity/Hub/Editor/6000.0.23f1/Editor/Unity.exe",
      workspaceRootOverride: "",
    },
    expandedTargetIds: {
      "target-1": true,
    },
    unityExecutableDiagnostics: buildUnityExecutableValidation(),
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
      workspaceRootOverride: "relative/workspace",
    },
  };
}

function buildPublishStepSnapshot(): CreateProjectWizardSnapshot {
  return {
    ...buildReviewSnapshot(),
    currentStepIndex: 3,
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
    unityExecutableDiagnostics: null,
  };
}

function buildEmptyUnityTargetsStepSnapshot(): CreateProjectWizardSnapshot {
  const snapshot = buildReviewSnapshot();

  return {
    ...snapshot,
    currentStepIndex: 2,
    draft: {
      ...snapshot.draft,
      buildTargets: [],
    },
    expandedTargetIds: {},
    unityExecutableDiagnostics: null,
  };
}

function buildTargetsStepSnapshotWithMultipleTargets(): CreateProjectWizardSnapshot {
  const snapshot = buildUnityTargetsStepSnapshot();

  return {
    ...snapshot,
    draft: {
      ...snapshot.draft,
      buildTargets: [
        {
          buildMethod: "HGPBuilder.PerformWindows64",
          id: "target-1",
          name: "Windows 64-bit",
          targetPlatform: "StandaloneWindows64",
        },
        {
          buildMethod: "HGPBuilder.PerformMacOS",
          id: "target-2",
          name: "macOS",
          targetPlatform: "StandaloneOSX",
        },
      ],
    },
    expandedTargetIds: {
      "target-1": true,
      "target-2": true,
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
    process_priority: "low" as const,
    runner_family: "host-native",
    status: "ready",
    unity_executable_exists: true,
    unity_executable_is_file: true,
    unity_executable_path:
      "C:/Program Files/Unity/Hub/Editor/6000.0.23f1/Editor/Unity.exe",
  };
}
