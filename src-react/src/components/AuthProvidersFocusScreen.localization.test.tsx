import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const {
  loadAuthProvidersMock,
  loadLocalizationSettingsMock,
  loginWithGithubAuthMock,
  loadSecretSettingsMock,
  readHostTextFileMock,
  saveSecretCredentialMock,
} = vi.hoisted(() => ({
  loadAuthProvidersMock: vi.fn(),
  loadLocalizationSettingsMock: vi.fn(),
  loginWithGithubAuthMock: vi.fn(),
  loadSecretSettingsMock: vi.fn(),
  readHostTextFileMock: vi.fn(),
  saveSecretCredentialMock: vi.fn(),
}));

vi.mock("../services/auth", () => ({
  loadAuthProviders: loadAuthProvidersMock,
  loginWithGithubAuth: loginWithGithubAuthMock,
}));

vi.mock("../services/projects", () => ({
  loadSecretSettings: loadSecretSettingsMock,
  saveSecretCredential: saveSecretCredentialMock,
}));

vi.mock("../services/runtime", () => ({
  loadLocalizationSettings: loadLocalizationSettingsMock,
}));

vi.mock("../services/processDetail", () => ({
  readHostTextFile: readHostTextFileMock,
}));

import { LocalizationProvider } from "../LocalizationProvider";
import { AuthProvidersFocusScreen } from "./AuthProvidersFocusScreen";
import OverlayProvider from "./OverlayManager";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

beforeEach(() => {
  loadAuthProvidersMock.mockResolvedValue([buildAuthProviderStatus()]);
  loginWithGithubAuthMock.mockResolvedValue(buildAuthProviderStatus());
  loadSecretSettingsMock.mockResolvedValue({
    credentials: [],
    storage_model: "sqlite-config-json-and-keyring-references",
    supported_credential_kinds: [
      "git-http-basic",
      "git-http-github-host-login",
      "itch-api-key",
    ],
    warnings: [],
  });
  saveSecretCredentialMock.mockResolvedValue(33);
  loadLocalizationSettingsMock.mockResolvedValue({
    available_locales: [
      {
        code: "en",
        display_name: "English",
        is_official: true,
        message_count: 20,
        native_name: "English",
      },
      {
        code: "pt-BR",
        display_name: "Português (Brasil)",
        is_official: true,
        message_count: 20,
        native_name: "Português (Brasil)",
      },
    ],
    fallback_locale: "en",
    localization_root: "C:/hgp/localizations",
    primary_locale: "pt-BR",
    warnings: [],
  });

  readHostTextFileMock.mockImplementation(async (path: string) => {
    if (path.endsWith("en.json")) {
      return buildHostTextFilePayload(
        path,
        JSON.stringify({
          messages: {
            "auth_provider_connection.modal.title":
              "{{providerLabel}} connection",
            "auth_provider_connection.progress.title": "Connection Stages",
            "auth_providers.actions.refresh": "Refresh providers",
            "auth_providers.inventory.title": "Available Accounts",
            "auth_providers.presentation.action.review_reconnect":
              "Review reconnect",
            "auth_providers.title": "Login Providers",
          },
        }),
      );
    }

    if (path.endsWith("pt-BR.json")) {
      return buildHostTextFilePayload(
        path,
        JSON.stringify({
          messages: {
            "auth_provider_connection.modal.title":
              "Conexão do {{providerLabel}}",
            "auth_provider_connection.progress.title": "Estágios da Conexão",
            "auth_providers.actions.refresh": "Atualizar provedores",
            "auth_providers.inventory.title": "Contas Disponíveis",
            "auth_providers.presentation.action.review_reconnect":
              "Revisar reconexão",
            "auth_providers.title": "Provedores de Login",
          },
        }),
      );
    }

    return buildHostTextFilePayload(path, JSON.stringify({ messages: {} }));
  });
});

describe("AuthProvidersFocusScreen localization", () => {
  it("renders translated auth-provider chrome from the official locale pack", async () => {
    render(
      <LocalizationProvider>
        <OverlayProvider>
          <AuthProvidersFocusScreen />
        </OverlayProvider>
      </LocalizationProvider>,
    );

    expect(
      await screen.findByRole("heading", { name: "Provedores de Login" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Atualizar provedores" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Contas Disponíveis")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Revisar reconexão" }));

    expect(
      await screen.findByRole("dialog", { name: "Conexão do GitHub" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Estágios da Conexão")).toBeInTheDocument();
  });
});

function buildAuthProviderStatus() {
  return {
    bound_repository_count: 2,
    credential_created_at: "2026-05-19T00:00:00Z",
    credential_id: 12,
    credential_name: "GitHub GCM",
    credential_updated_at: "2026-05-19T00:12:00Z",
    instance_url: "https://github.com",
    label: "GitHub",
    provider_id: "github.com",
    status: "connected",
    status_message: "GitHub access is connected and ready.",
  };
}

function buildHostTextFilePayload(path: string, content: string) {
  return {
    content,
    exists: true,
    path,
    size_bytes: content.length,
    truncated: false,
  };
}
