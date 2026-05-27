import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const {
  loadAuthProvidersMock,
  loadSecretSettingsMock,
  openOverlayMock,
  saveSecretCredentialMock,
} = vi.hoisted(() => ({
  loadAuthProvidersMock: vi.fn(),
  loadSecretSettingsMock: vi.fn(),
  openOverlayMock: vi.fn(),
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

vi.mock("../services/auth", () => ({
  loadAuthProviders: loadAuthProvidersMock,
}));

vi.mock("../services/projects", () => ({
  loadSecretSettings: loadSecretSettingsMock,
  saveSecretCredential: saveSecretCredentialMock,
}));

import { AuthProvidersFocusScreen } from "./AuthProvidersFocusScreen";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

beforeEach(() => {
  loadAuthProvidersMock.mockResolvedValue([buildAuthProviderStatus()]);
  loadSecretSettingsMock.mockResolvedValue(buildSecretSettings());
  openOverlayMock.mockResolvedValue(null);
  saveSecretCredentialMock.mockResolvedValue(33);
});

describe("AuthProvidersFocusScreen credential inventory", () => {
  it("creates a reusable Itch credential from the auth screen and refreshes the inventory", async () => {
    loadSecretSettingsMock
      .mockResolvedValueOnce(
        buildSecretSettings({
          credentials: [],
        }),
      )
      .mockResolvedValueOnce(
        buildSecretSettings({
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
        }),
      );

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

    render(<AuthProvidersFocusScreen />);

    await screen.findByRole("heading", { name: "Login Providers" });

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

  it("updates an existing Itch credential from the auth screen and refreshes the inventory", async () => {
    loadSecretSettingsMock
      .mockResolvedValueOnce(
        buildSecretSettings({
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
        }),
      )
      .mockResolvedValueOnce(
        buildSecretSettings({
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
        }),
      );

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

    render(<AuthProvidersFocusScreen />);

    await screen.findByRole("heading", { name: "Login Providers" });

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
    expect(await screen.findByText("Itch global")).toBeInTheDocument();
    expect(await screen.findByText("itch-api-key")).toBeInTheDocument();
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

function buildSecretSettings(
  overrides?: Partial<{
    credentials: Array<{
      config_summary: {
        message: string;
        missing_required_keys: string[];
        status: string;
        top_level_keys: string[];
      };
      created_at: string;
      credential_id: number;
      kind: string;
      name: string;
      storage_model: string;
      updated_at: string;
    }>;
    storage_model: string;
    supported_credential_kinds: string[];
    warnings: string[];
  }>,
) {
  return {
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
    ...overrides,
  };
}
