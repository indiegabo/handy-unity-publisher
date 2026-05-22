import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
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
  removeRepositoryProjectMock,
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
  removeRepositoryProjectMock: vi.fn(),
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
  removeRepositoryProject: removeRepositoryProjectMock,
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
  removeRepositoryProjectMock.mockResolvedValue(buildProjectRemovalReport());
  reconnectRepositoryAuthMock.mockResolvedValue(undefined);
  saveSecretCredentialMock.mockResolvedValue(303);
  updateRepositoryProjectMock.mockResolvedValue(undefined);
  validateUnityExecutablePathMock.mockResolvedValue(
    buildUnityExecutableValidation(),
  );
});

describe("RepositoryProjectDetail", () => {
  it("shows a retryable error state when the project detail load fails", async () => {
    loadRepositoryProjectDetailMock
      .mockRejectedValueOnce(new Error("Detail offline"))
      .mockResolvedValue(buildRepositoryDetail());

    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        repositoryId={1}
      />,
    );

    expect(
      await screen.findByText("Project detail is unavailable."),
    ).toBeInTheDocument();
    expect(screen.getByText("Detail offline")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry project load" }));

    expect(
      await screen.findByRole("heading", { name: "Revolutions" }),
    ).toBeInTheDocument();
  });

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

  it("supports ArrowUp, ArrowDown, Home, and End navigation across project section tabs", async () => {
    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        repositoryId={1}
      />,
    );

    expect(
      await screen.findByRole("heading", { name: "Revolutions" }),
    ).toBeInTheDocument();

    const projectSettingsTab = await screen.findByRole("tab", {
      name: "Project Settings",
    });

    projectSettingsTab.focus();
    fireEvent.keyDown(projectSettingsTab, { key: "ArrowDown" });

    await waitFor(() => {
      const repositoryTab = screen.getByRole("tab", { name: "Repository" });

      expect(repositoryTab).toHaveFocus();
      expect(repositoryTab).toHaveAttribute("aria-selected", "true");
    });

    fireEvent.keyDown(screen.getByRole("tab", { name: "Repository" }), {
      key: "End",
    });

    await waitFor(() => {
      const runtimeStatusTab = screen.getByRole("tab", {
        name: "Runtime Status",
      });

      expect(runtimeStatusTab).toHaveFocus();
      expect(runtimeStatusTab).toHaveAttribute("aria-selected", "true");
    });

    fireEvent.keyDown(screen.getByRole("tab", { name: "Runtime Status" }), {
      key: "Home",
    });

    await waitFor(() => {
      const refreshedProjectSettingsTab = screen.getByRole("tab", {
        name: "Project Settings",
      });

      expect(refreshedProjectSettingsTab).toHaveFocus();
      expect(refreshedProjectSettingsTab).toHaveAttribute(
        "aria-selected",
        "true",
      );
    });

    fireEvent.keyDown(screen.getByRole("tab", { name: "Project Settings" }), {
      key: "ArrowUp",
    });

    await waitFor(() => {
      const runtimeStatusTab = screen.getByRole("tab", {
        name: "Runtime Status",
      });

      expect(runtimeStatusTab).toHaveFocus();
      expect(runtimeStatusTab).toHaveAttribute("aria-selected", "true");
    });
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

  it("shows a stable collapsed summary before reopening a build target editor", async () => {
    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        repositoryId={1}
      />,
    );

    fireEvent.click(await screen.findByRole("tab", { name: "Build Targets" }));

    expect(
      await screen.findByText("Builder.PerformWindows"),
    ).toBeInTheDocument();
    expect(screen.getByText("Ready")).toBeInTheDocument();
    expect(screen.getByText("No publish bindings")).toBeInTheDocument();
    expect(
      screen.queryByLabelText("Unity build method"),
    ).not.toBeInTheDocument();
    expect(
      document.querySelector(
        ".project-detail-target-card__summary-strip.ui-summary-strip",
      ),
    ).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Edit" }));

    expect(
      await screen.findByLabelText("Unity build method"),
    ).toBeInTheDocument();
  });

  it("uses shared summary strips in project detail section headers", async () => {
    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        repositoryId={1}
      />,
    );

    fireEvent.click(await screen.findByRole("tab", { name: "Build Targets" }));

    const buildTargetsPanel = screen
      .getByRole("heading", { name: "Build Targets" })
      .closest("section");

    expect(buildTargetsPanel).not.toBeNull();
    expect(
      (buildTargetsPanel as HTMLElement).querySelector(
        ".project-detail-section-accordion__summary.ui-summary-strip",
      ),
    ).not.toBeNull();
  });

  it("uses consistent build-target removal copy when publish bindings would also be removed", async () => {
    loadRepositoryProjectDetailMock.mockResolvedValue(
      buildRepositoryDetail({
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
          {
            build_target_id: 12,
            diagnostic_message: "Ready for host-native execution.",
            diagnostic_status: "ready",
            enabled: true,
            host_native_diagnostics: buildUnityExecutableValidation(),
            repository_id: 1,
            repository_name: "Revolutions",
            runner_type: "host-native",
            target_name: "Linux",
            unity_build_method: "Builder.PerformLinux",
            unity_target_platform: "StandaloneLinux64",
          },
        ],
        enabled_build_target_count: 2,
        publish_targets: [
          buildItchPublishTarget({
            channel: "windows-stable",
            gameSlug: "red-horizon",
          }),
        ],
      }),
    );

    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        repositoryId={1}
      />,
    );

    fireEvent.click(await screen.findByRole("tab", { name: "Build Targets" }));

    fireEvent.click(
      await screen.findByRole("button", { name: "Remove build target 1" }),
    );

    expect(
      await screen.findByText("Confirm build target removal"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "Remove build target and bindings",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/also removes publish bindings from/i),
    ).toBeInTheDocument();
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

  it("uses a shared summary strip for repository access support metadata", async () => {
    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        repositoryId={1}
      />,
    );

    fireEvent.click(await screen.findByRole("tab", { name: "Repository" }));

    const repositoryAccessCallout = screen
      .getByText("Repository access")
      .closest(".wizard-callout");

    expect(repositoryAccessCallout).not.toBeNull();
    expect(
      (repositoryAccessCallout as HTMLElement).querySelector(
        ".wizard-callout__summary-strip.ui-summary-strip",
      ),
    ).not.toBeNull();
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

    expect(
      await screen.findByRole("heading", { name: "Draft impact" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Unbound targets")).toBeInTheDocument();
  });

  it("keeps a stable focus order through the publish destination accordion and binding controls", async () => {
    loadRepositoryProjectDetailMock.mockResolvedValue(
      buildRepositoryDetail({
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
          {
            build_target_id: 12,
            diagnostic_message: "Ready for host-native execution.",
            diagnostic_status: "ready",
            enabled: true,
            host_native_diagnostics: buildUnityExecutableValidation(),
            repository_id: 1,
            repository_name: "Revolutions",
            runner_type: "host-native",
            target_name: "Linux",
            unity_build_method: "Builder.PerformLinux",
            unity_target_platform: "StandaloneLinux64",
          },
        ],
        enabled_build_target_count: 2,
        publish_targets: [
          buildItchPublishTarget({
            channel: "windows-stable",
            gameSlug: "red-horizon",
          }),
        ],
      }),
    );

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

    const panel = await screen.findByRole("tabpanel", {
      name: "Publish Destinations",
    });
    const itchAccordion = (
      await within(panel).findByRole("heading", { name: "Itch" })
    ).closest(".vertical-accordion");

    expect(itchAccordion).not.toBeNull();

    if (itchAccordion?.getAttribute("data-state") !== "open") {
      fireEvent.click(
        within(itchAccordion as HTMLElement).getByRole("button", {
          name: "Expand section",
        }),
      );
    }

    const [destinationStatusSelect, bindingStatusSelect] = within(
      panel,
    ).getAllByRole("combobox", { name: "Status" });

    expectFocusOrder(panel, [
      within(panel).getByRole("button", { name: "Add destination" }),
      within(itchAccordion as HTMLElement).getByRole("button", {
        name: "Collapse section",
      }),
      within(itchAccordion as HTMLElement).getByRole("button", {
        name: "Remove Itch destination",
      }),
      destinationStatusSelect,
      within(panel).getByRole("textbox", { name: "Itch account name" }),
      within(panel).getByRole("textbox", { name: "Itch game slug" }),
      within(panel).getByRole("button", { name: "New credential" }),
      within(panel).getByRole("combobox", { name: "Credential" }),
      within(panel).getByRole("combobox", { name: "Target" }),
      within(panel).getByRole("button", { name: "Add target" }),
      within(panel).getByRole("button", { name: "Remove binding for Windows" }),
      bindingStatusSelect,
      within(panel).getByRole("textbox", { name: "Itch channel" }),
      within(panel).getByRole("textbox", {
        name: "Itch userversion template",
      }),
    ]);
  });

  it("persists edited publish destination fields through project save", async () => {
    loadRepositoryProjectDetailMock.mockReset();
    loadRepositoryProjectDetailMock
      .mockResolvedValueOnce(
        buildRepositoryDetail({
          publish_targets: [
            buildItchPublishTarget({
              channel: "windows-stable",
              gameSlug: "red-horizon",
            }),
          ],
        }),
      )
      .mockResolvedValueOnce(
        buildRepositoryDetail({
          publish_targets: [
            buildItchPublishTarget({
              channel: "windows-stable",
              gameSlug: "red-horizon-redux",
            }),
          ],
        }),
      );

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

    const panel = await screen.findByRole("tabpanel", {
      name: "Publish Destinations",
    });
    const itchAccordion = (
      await within(panel).findByRole("heading", { name: "Itch" })
    ).closest(".vertical-accordion");

    expect(itchAccordion).not.toBeNull();

    if (itchAccordion?.getAttribute("data-state") !== "open") {
      fireEvent.click(
        within(itchAccordion as HTMLElement).getByRole("button", {
          name: "Expand section",
        }),
      );
    }

    const gameSlugField = await screen.findByLabelText("Itch game slug");

    fireEvent.change(gameSlugField, {
      target: { value: "red-horizon-redux" },
    });

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Save Changes" }),
      ).toBeEnabled();
    });

    fireEvent.click(screen.getByRole("button", { name: "Save Changes" }));

    await waitFor(() => {
      expect(updateRepositoryProjectMock).toHaveBeenCalledWith(
        expect.objectContaining({
          repository_id: 1,
          publish_targets: [
            expect.objectContaining({
              bindings: [
                expect.objectContaining({
                  build_target_id: 11,
                  build_target_name: "Windows",
                  enabled: true,
                  options_json: JSON.stringify({
                    channel: "windows-stable",
                    userversion_template: "{{git_tag}}",
                  }),
                }),
              ],
              config_json: JSON.stringify({
                account_name: "indiegabo",
                game_slug: "red-horizon-redux",
              }),
              credentials_id: 202,
              enabled: true,
              kind: "itch",
              name: "Itch",
              publish_target_id: 5,
            }),
          ],
        }),
      );
    });

    await waitFor(() => {
      expect(screen.getByLabelText("Itch game slug")).toHaveValue(
        "red-horizon-redux",
      );
      expect(
        screen.getByRole("button", { name: "Save Changes" }),
      ).toBeDisabled();
    });
  });

  it("shows the saved publish credential name instead of the current-id fallback", async () => {
    loadRepositoryProjectDetailMock.mockResolvedValue(
      buildRepositoryDetail({
        publish_targets: [
          {
            ...buildItchPublishTarget({
              channel: "windows-stable",
              gameSlug: "red-horizon",
            }),
            credentials: {
              config_message:
                "stored credential config_json is missing required keys: api_key",
              config_status: "incomplete_config",
              credential_id: 303,
              kind: "itch-api-key",
              name: "My Key",
            },
          },
        ],
      }),
    );
    loadSecretSettingsMock.mockResolvedValue(
      buildSecretSettings({
        credentials: [
          ...buildSecretSettings().credentials,
          buildItchSecretCredential({
            credential_id: 303,
            name: "My Key",
            status: "incomplete_config",
            message:
              "stored credential config_json is missing required keys: api_key",
            top_level_keys: [],
            missing_required_keys: ["api_key"],
          }),
        ],
      }),
    );

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

    const itchAccordion = (
      await screen.findByRole("heading", { name: "Itch" })
    ).closest(".vertical-accordion");

    expect(itchAccordion).not.toBeNull();

    if (itchAccordion?.getAttribute("data-state") !== "open") {
      fireEvent.click(
        within(itchAccordion as HTMLElement).getByRole("button", {
          name: "Expand section",
        }),
      );
    }

    await waitFor(() => {
      expect(screen.getByRole("combobox", { name: "Credential" })).toHaveValue(
        "303",
      );
      expect(
        screen.getByRole("combobox", { name: "Credential" }),
      ).toHaveDisplayValue("My Key");
    });
  });

  it("keeps publish validation errors in the destinations section and blocks save", async () => {
    loadRepositoryProjectDetailMock.mockResolvedValue(
      buildRepositoryDetail({
        publish_targets: [
          buildItchPublishTarget({
            channel: "windows-stable",
            gameSlug: "red-horizon",
          }),
        ],
      }),
    );

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

    const itchAccordion = (
      await screen.findByRole("heading", { name: "Itch" })
    ).closest(".vertical-accordion");

    expect(itchAccordion).not.toBeNull();

    if (itchAccordion?.getAttribute("data-state") !== "open") {
      fireEvent.click(
        within(itchAccordion as HTMLElement).getByRole("button", {
          name: "Expand section",
        }),
      );
    }

    fireEvent.change(await screen.findByLabelText("Itch game slug"), {
      target: { value: "" },
    });

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Save Changes" }),
      ).toBeEnabled();
    });

    fireEvent.click(screen.getByRole("button", { name: "Save Changes" }));

    expect(
      await screen.findByText("Itch game slug is required."),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("tab", { name: "Publish Destinations" }),
    ).toHaveAttribute("aria-selected", "true");
    expect(updateRepositoryProjectMock).not.toHaveBeenCalled();
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

  it("removes the project from the app only and notifies the caller", async () => {
    const onProjectRemoved = vi.fn();

    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        onProjectRemoved={onProjectRemoved}
        repositoryId={1}
      />,
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "Remove Project" }),
    );

    expect(
      await screen.findByRole("dialog", { name: "Remove Revolutions?" }),
    ).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "Remove from App Only" }),
    );

    await waitFor(() => {
      expect(removeRepositoryProjectMock).toHaveBeenCalledWith({
        repository_id: 1,
        strategy: "detach",
      });
    });

    await waitFor(() => {
      expect(onProjectRemoved).toHaveBeenCalledWith(
        expect.objectContaining({
          repository_id: 1,
          strategy: "detach",
        }),
      );
    });
  });

  it("purges project runtime files when purge total is selected", async () => {
    removeRepositoryProjectMock.mockResolvedValue(
      buildProjectRemovalReport({ strategy: "purge" }),
    );

    render(
      <RepositoryProjectDetail
        onProjectNameResolved={vi.fn()}
        onProjectRemoved={vi.fn()}
        repositoryId={1}
      />,
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "Remove Project" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Purge Total" }));

    await waitFor(() => {
      expect(removeRepositoryProjectMock).toHaveBeenCalledWith({
        repository_id: 1,
        strategy: "purge",
      });
    });
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
    expect(
      screen.getByRole("button", { name: "Remove Project" }),
    ).toBeDisabled();

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

function buildSecretSettings(
  overrides: Partial<{
    credentials: Array<Record<string, unknown>>;
    storage_model: string;
    supported_credential_kinds: string[];
    warnings: string[];
  }> = {},
) {
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
    ...overrides,
  };
}

function buildItchSecretCredential({
  credential_id,
  message,
  missing_required_keys,
  name,
  status,
  top_level_keys,
}: {
  credential_id: number;
  message: string;
  missing_required_keys: string[];
  name: string;
  status: string;
  top_level_keys: string[];
}) {
  return {
    config_summary: {
      message,
      missing_required_keys,
      status,
      top_level_keys,
    },
    created_at: "2026-05-19T00:00:00Z",
    credential_id,
    kind: "itch-api-key",
    name,
    storage_model: "sqlite-config-json-and-keyring-references",
    updated_at: "2026-05-19T00:00:00Z",
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

function buildItchPublishTarget({
  channel,
  gameSlug,
}: {
  channel: string;
  gameSlug: string;
}) {
  return {
    bindings: [
      {
        build_target_id: 11,
        build_target_name: "Windows",
        consumption_behavior: "non_consuming",
        enabled: true,
        options_json: JSON.stringify({
          channel,
          userversion_template: "{{git_tag}}",
        }),
      },
    ],
    config_json: JSON.stringify({
      account_name: "indiegabo",
      game_slug: gameSlug,
    }),
    credentials: {
      config_message: "Itch API key is present.",
      config_status: "ready",
      credential_id: 202,
      kind: "itch-api-key",
      name: "Itch Release",
    },
    enabled: true,
    kind: "itch",
    name: "Itch stable",
    publish_target_id: 5,
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

function expectFocusOrder(container: HTMLElement, elements: HTMLElement[]) {
  const focusableElements = listFocusableElements(container);
  let previousIndex = -1;

  for (const element of elements) {
    const nextIndex = focusableElements.indexOf(element);

    expect(nextIndex).toBeGreaterThan(previousIndex);
    previousIndex = nextIndex;
  }
}

function listFocusableElements(container: HTMLElement) {
  return Array.from(
    container.querySelectorAll<HTMLElement>(
      [
        "button:not([disabled])",
        "input:not([disabled])",
        "select:not([disabled])",
        "textarea:not([disabled])",
        '[tabindex]:not([tabindex="-1"])',
      ].join(", "),
    ),
  ).filter(
    (element) =>
      !element.hidden && !element.closest("[hidden], [aria-hidden='true']"),
  );
}
