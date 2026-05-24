import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const {
  loadApplicationVersionMock,
  loadLocalizationSettingsMock,
  loadRuntimeDirectoriesMock,
  loadRuntimeHealthMock,
  loadSecretSettingsMock,
  openOverlayMock,
  openExternalUrlMock,
  openHostPathMock,
  saveLocalizationPreferencesMock,
  saveSecretCredentialMock,
} = vi.hoisted(() => ({
  loadApplicationVersionMock: vi.fn(),
  loadLocalizationSettingsMock: vi.fn(),
  loadRuntimeDirectoriesMock: vi.fn(),
  loadRuntimeHealthMock: vi.fn(),
  loadSecretSettingsMock: vi.fn(),
  openOverlayMock: vi.fn(),
  openExternalUrlMock: vi.fn(),
  openHostPathMock: vi.fn(),
  saveLocalizationPreferencesMock: vi.fn(),
  saveSecretCredentialMock: vi.fn(),
}));

vi.mock("./OverlayManager", () => ({
  __esModule: true,
  default: ({ children }: { children: unknown }) => children,
  useOverlay: () => ({
    dismissTopOverlay: vi.fn(),
    hasOpenOverlay: false,
    openOverlay: openOverlayMock,
  }),
}));

vi.mock("../services/runtime", () => ({
  loadApplicationVersion: loadApplicationVersionMock,
  loadLocalizationSettings: loadLocalizationSettingsMock,
  loadRuntimeDirectories: loadRuntimeDirectoriesMock,
  loadRuntimeHealth: loadRuntimeHealthMock,
  openExternalUrl: openExternalUrlMock,
  openHostPath: openHostPathMock,
  saveLocalizationPreferences: saveLocalizationPreferencesMock,
}));

vi.mock("../services/projects", () => ({
  loadSecretSettings: loadSecretSettingsMock,
  saveSecretCredential: saveSecretCredentialMock,
}));

import { SettingsFocusScreen } from "./SettingsFocusScreen";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

beforeEach(() => {
  loadApplicationVersionMock.mockResolvedValue({
    app_version: "0.1.0",
    product_name: "HGP",
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
  loadRuntimeDirectoriesMock.mockResolvedValue({
    artifacts_dir: "C:/Users/indie/AppData/Roaming/HGP/data/artifacts",
    data_dir: "C:/Users/indie/AppData/Roaming/HGP/data",
    database_path: "C:/Users/indie/AppData/Roaming/HGP/data/runtime.sqlite3",
    health_report_path:
      "C:/Users/indie/AppData/Roaming/HGP/data/runtime/health.json",
    logs_dir: "C:/Users/indie/AppData/Roaming/HGP/data/logs",
    runs_dir: "C:/Users/indie/AppData/Roaming/HGP/data/runs",
    runtime_events_cursor_path:
      "C:/Users/indie/AppData/Roaming/HGP/data/runtime/events.cursor",
    runtime_events_path:
      "C:/Users/indie/AppData/Roaming/HGP/data/runtime/events.jsonl",
    runtime_log_path:
      "C:/Users/indie/AppData/Roaming/HGP/data/logs/runtime.log",
    state_dir: "C:/Users/indie/AppData/Roaming/HGP/data/runtime",
    supervision_contract_path:
      "C:/Users/indie/AppData/Roaming/HGP/data/runtime/supervision.json",
    supervisor_state_path:
      "C:/Users/indie/AppData/Roaming/HGP/data/runtime/supervisor-state.json",
  });
  loadLocalizationSettingsMock.mockResolvedValue(buildLocalizationSettings());
  loadSecretSettingsMock.mockResolvedValue({
    credentials: [
      {
        config_summary: {
          message: "Stored GitHub login metadata is valid.",
          missing_required_keys: [],
          status: "ready",
          top_level_keys: ["provider", "instance_url"],
        },
        created_at: "2026-05-19T00:00:00Z",
        credential_id: 7,
        kind: "git-http-github-host-login",
        name: "GitHub.com",
        storage_model: "sqlite-config-json-and-keyring-references",
        updated_at: "2026-05-19T00:00:00Z",
      },
    ],
    storage_model: "sqlite-config-json-and-keyring-references",
    supported_credential_kinds: [
      "git-http-basic",
      "git-http-github-host-login",
      "itch-api-key",
    ],
    warnings: [],
  });
  openOverlayMock.mockResolvedValue(null);
  openExternalUrlMock.mockResolvedValue(undefined);
  openHostPathMock.mockResolvedValue(undefined);
  saveLocalizationPreferencesMock.mockResolvedValue(
    buildLocalizationSettings(),
  );
  saveSecretCredentialMock.mockResolvedValue(33);
});

describe("SettingsFocusScreen", () => {
  it("renders the shell snapshot and opens a runtime storage path", async () => {
    render(
      <SettingsFocusScreen
        automationMode="active"
        onManageAuthProviders={vi.fn()}
        onOpenProjects={vi.fn()}
        onOpenProjectWorkers={vi.fn()}
      />,
    );

    expect(
      await screen.findByRole("heading", { name: "Settings" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByText("handy-games-publisher-runtime"),
    ).toBeInTheDocument();
    expect(await screen.findByText("GitHub.com")).toBeInTheDocument();

    fireEvent.click(
      await screen.findByRole("button", { name: "Open data directory" }),
    );

    await waitFor(() => {
      expect(openHostPathMock).toHaveBeenCalledWith(
        "C:/Users/indie/AppData/Roaming/HGP/data",
      );
    });
  });

  it("exposes the global navigation entry points", async () => {
    const manageAuthProviders = vi.fn();
    const openProjects = vi.fn();
    const openProjectWorkers = vi.fn();

    render(
      <SettingsFocusScreen
        automationMode="idle"
        onManageAuthProviders={manageAuthProviders}
        onOpenProjects={openProjects}
        onOpenProjectWorkers={openProjectWorkers}
      />,
    );

    await screen.findByRole("heading", { name: "Settings" });

    fireEvent.click(
      screen.getByRole("button", { name: "Manage auth providers" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Open projects" }));
    fireEvent.click(
      screen.getByRole("button", { name: "Open project workers" }),
    );

    expect(manageAuthProviders).toHaveBeenCalledTimes(1);
    expect(openProjects).toHaveBeenCalledTimes(1);
    expect(openProjectWorkers).toHaveBeenCalledTimes(1);
  });

  it("saves shell localization preferences from the settings screen", async () => {
    saveLocalizationPreferencesMock.mockResolvedValueOnce(
      buildLocalizationSettings({
        fallback_locale: "pt-BR",
        primary_locale: "pt-BR",
      }),
    );

    render(
      <SettingsFocusScreen
        automationMode="active"
        onManageAuthProviders={vi.fn()}
        onOpenProjects={vi.fn()}
        onOpenProjectWorkers={vi.fn()}
      />,
    );

    await screen.findByRole("heading", { name: "Settings" });
    const primaryLanguageField =
      await screen.findByLabelText("Primary language");

    fireEvent.change(primaryLanguageField, {
      target: { value: "pt-BR" },
    });

    await waitFor(() => {
      expect(saveLocalizationPreferencesMock).toHaveBeenCalledWith({
        fallback_locale: "pt-BR",
        primary_locale: "pt-BR",
      });
    });

    expect(
      await screen.findByText(
        "Language preferences saved. The shell will use the updated primary and fallback locale selection as translated surfaces adopt the locale contract.",
      ),
    ).toBeInTheDocument();
    expect(primaryLanguageField).toHaveValue("pt-BR");
  });

  it("opens the public beta documentation from settings", async () => {
    render(
      <SettingsFocusScreen
        automationMode="active"
        onManageAuthProviders={vi.fn()}
        onOpenProjects={vi.fn()}
        onOpenProjectWorkers={vi.fn()}
      />,
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "Open beta docs" }),
    );

    await waitFor(() => {
      expect(openExternalUrlMock).toHaveBeenCalledWith(
        "https://indiegabo.github.io/handy-games-publisher/",
      );
    });
  });

  it("creates a reusable Itch credential from settings and refreshes the inventory", async () => {
    loadSecretSettingsMock
      .mockResolvedValueOnce({
        credentials: [],
        storage_model: "sqlite-config-json-and-keyring-references",
        supported_credential_kinds: [
          "git-http-basic",
          "git-http-github-host-login",
          "itch-api-key",
        ],
        warnings: [],
      })
      .mockResolvedValueOnce({
        credentials: [
          {
            config_summary: {
              message: "Stored Itch API key is valid.",
              missing_required_keys: [],
              status: "ready",
              top_level_keys: ["api_key"],
            },
            created_at: "2026-05-23T00:00:00Z",
            credential_id: 33,
            kind: "itch-api-key",
            name: "Itch global",
            storage_model: "sqlite-config-json-and-keyring-references",
            updated_at: "2026-05-23T00:00:00Z",
          },
        ],
        storage_model: "sqlite-config-json-and-keyring-references",
        supported_credential_kinds: [
          "git-http-basic",
          "git-http-github-host-login",
          "itch-api-key",
        ],
        warnings: [],
      });

    openOverlayMock.mockImplementationOnce(async (_Component, props) => {
      const input = {
        credential_id: null,
        config_json: JSON.stringify({
          api_key: "itch-token-123",
        }),
        kind: "itch-api-key",
        name: "Itch global",
      };

      await props.onSubmit(input);
      return input;
    });

    render(
      <SettingsFocusScreen
        automationMode="active"
        onManageAuthProviders={vi.fn()}
        onOpenProjects={vi.fn()}
        onOpenProjectWorkers={vi.fn()}
      />,
    );

    await screen.findByRole("heading", { name: "Settings" });

    fireEvent.click(
      await screen.findByRole("button", { name: "New Itch credential" }),
    );

    await waitFor(() => {
      expect(openOverlayMock).toHaveBeenCalledTimes(1);
      expect(saveSecretCredentialMock).toHaveBeenCalledWith({
        credential_id: null,
        config_json: JSON.stringify({
          api_key: "itch-token-123",
        }),
        kind: "itch-api-key",
        name: "Itch global",
      });
    });

    expect(
      await screen.findByText(
        "Reusable Itch credential saved. It can now be selected from publish destinations.",
      ),
    ).toBeInTheDocument();
    expect(await screen.findByText("Itch global")).toBeInTheDocument();
  });

  it("updates an existing Itch credential from settings and refreshes the inventory", async () => {
    loadSecretSettingsMock
      .mockResolvedValueOnce({
        credentials: [
          {
            config_summary: {
              message: "Stored Itch API key needs refresh.",
              missing_required_keys: ["api_key"],
              status: "missing_required_keys",
              top_level_keys: [],
            },
            created_at: "2026-05-23T00:00:00Z",
            credential_id: 33,
            kind: "itch-api-key",
            name: "Itch global",
            storage_model: "sqlite-config-json-and-keyring-references",
            updated_at: "2026-05-23T00:00:00Z",
          },
        ],
        storage_model: "sqlite-config-json-and-keyring-references",
        supported_credential_kinds: [
          "git-http-basic",
          "git-http-github-host-login",
          "itch-api-key",
        ],
        warnings: [],
      })
      .mockResolvedValueOnce({
        credentials: [
          {
            config_summary: {
              message: "Stored Itch API key is valid.",
              missing_required_keys: [],
              status: "ready",
              top_level_keys: ["api_key"],
            },
            created_at: "2026-05-23T00:00:00Z",
            credential_id: 33,
            kind: "itch-api-key",
            name: "Itch global",
            storage_model: "sqlite-config-json-and-keyring-references",
            updated_at: "2026-05-24T00:00:00Z",
          },
        ],
        storage_model: "sqlite-config-json-and-keyring-references",
        supported_credential_kinds: [
          "git-http-basic",
          "git-http-github-host-login",
          "itch-api-key",
        ],
        warnings: [],
      });

    openOverlayMock.mockImplementationOnce(async (_Component, props) => {
      expect(props.initialCredential).toEqual({
        credentialId: 33,
        kind: "itch-api-key",
        name: "Itch global",
      });

      const input = {
        credential_id: 33,
        config_json: JSON.stringify({
          api_key: "itch-token-456",
        }),
        kind: "itch-api-key",
        name: "Itch global",
      };

      await props.onSubmit(input);
      return input;
    });

    render(
      <SettingsFocusScreen
        automationMode="active"
        onManageAuthProviders={vi.fn()}
        onOpenProjects={vi.fn()}
        onOpenProjectWorkers={vi.fn()}
      />,
    );

    await screen.findByRole("heading", { name: "Settings" });

    fireEvent.click(
      await screen.findByRole("button", { name: "Edit Itch global" }),
    );

    await waitFor(() => {
      expect(openOverlayMock).toHaveBeenCalledTimes(1);
      expect(saveSecretCredentialMock).toHaveBeenCalledWith({
        credential_id: 33,
        config_json: JSON.stringify({
          api_key: "itch-token-456",
        }),
        kind: "itch-api-key",
        name: "Itch global",
      });
    });

    expect(
      await screen.findByText(
        "Reusable Itch credential updated. Publish destinations will use the refreshed secret on the next run.",
      ),
    ).toBeInTheDocument();
    expect(
      await screen.findByText("Stored Itch API key is valid."),
    ).toBeInTheDocument();
  });
});

function buildLocalizationSettings(
  overrides?: Partial<{
    available_locales: Array<{
      code: string;
      display_name: string;
      is_official: boolean;
      message_count: number;
      native_name: string;
    }>;
    fallback_locale: string;
    localization_root: string;
    primary_locale: string;
    warnings: string[];
  }>,
) {
  return {
    available_locales: [
      {
        code: "en",
        display_name: "English",
        is_official: true,
        message_count: 3,
        native_name: "English",
      },
      {
        code: "pt-BR",
        display_name: "Brazilian Portuguese",
        is_official: true,
        message_count: 3,
        native_name: "Português (Brasil)",
      },
    ],
    fallback_locale: "pt-BR",
    localization_root:
      "C:/Users/indie/projetos/Apps/handy-unity-publisher/apps/desktop/src-tauri/localizations",
    primary_locale: "en",
    warnings: [],
    ...overrides,
  };
}
