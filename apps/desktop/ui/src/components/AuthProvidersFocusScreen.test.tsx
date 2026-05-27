import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AuthProvidersFocusScreen } from "./AuthProvidersFocusScreen";
import OverlayProvider from "./OverlayManager";

const {
  loadAuthProvidersMock,
  loadSecretSettingsMock,
  loginWithGithubAuthMock,
  saveSecretCredentialMock,
} = vi.hoisted(() => ({
  loadAuthProvidersMock: vi.fn(),
  loadSecretSettingsMock: vi.fn(),
  loginWithGithubAuthMock: vi.fn(),
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

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

beforeEach(() => {
  loadAuthProvidersMock.mockResolvedValue([buildAuthProviderStatus()]);
  loadSecretSettingsMock.mockResolvedValue(buildSecretSettings());
  loginWithGithubAuthMock.mockResolvedValue(buildAuthProviderStatus());
  saveSecretCredentialMock.mockResolvedValue(33);
});

describe("AuthProvidersFocusScreen", () => {
  it("uses the shared focus-screen header contract for page identity and summary", async () => {
    render(
      <OverlayProvider>
        <AuthProvidersFocusScreen />
      </OverlayProvider>,
    );

    expect(
      await screen.findByRole("heading", { name: "Login Providers" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("heading", { name: "GitHub" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Refresh providers" }),
    ).toBeInTheDocument();
  });

  it("uses shared summary strips for provider cards and the connection overlay", async () => {
    render(
      <OverlayProvider>
        <AuthProvidersFocusScreen />
      </OverlayProvider>,
    );

    await screen.findByRole("heading", { name: "GitHub" });

    fireEvent.click(
      screen.getByRole("button", {
        name: /Review (connection|reconnect)/i,
      }),
    );

    const dialog = await screen.findByRole("dialog", {
      name: "GitHub connection",
    });

    expect(
      dialog.querySelector(".auth-provider-card__summary-strip"),
    ).not.toBeNull();
  });

  it("renders a retryable unavailable state instead of a false empty provider inventory", async () => {
    loadAuthProvidersMock.mockRejectedValueOnce(new Error("Provider offline"));

    render(
      <OverlayProvider>
        <AuthProvidersFocusScreen />
      </OverlayProvider>,
    );

    expect(
      await screen.findByRole("button", { name: "Retry provider load" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("No login providers are available."),
    ).not.toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "Retry provider load" }),
    );

    expect(
      await screen.findByRole("heading", { name: "GitHub" }),
    ).toBeInTheDocument();
  });

  it("keeps the last provider inventory visible when a refresh fails", async () => {
    render(
      <OverlayProvider>
        <AuthProvidersFocusScreen />
      </OverlayProvider>,
    );

    expect(
      await screen.findByRole("heading", { name: "GitHub" }),
    ).toBeInTheDocument();

    loadAuthProvidersMock.mockRejectedValueOnce(new Error("Provider offline"));

    fireEvent.click(screen.getByRole("button", { name: "Refresh providers" }));

    expect(await screen.findByText("Provider offline")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "GitHub" })).toBeInTheDocument();
  });

  it("opens the auth overlay and updates provider state after a successful reconnect", async () => {
    const onResult = vi.fn();

    render(
      <OverlayProvider>
        <AuthProvidersFocusScreen onResult={onResult} />
      </OverlayProvider>,
    );

    fireEvent.click(
      await screen.findByRole("button", {
        name: /Review (connection|reconnect)/i,
      }),
    );

    const dialog = await screen.findByRole("dialog", {
      name: "GitHub connection",
    });

    fireEvent.click(within(dialog).getByRole("button", { name: "Continue" }));

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Reconnect with browser" }),
    );

    await waitFor(() => {
      expect(loginWithGithubAuthMock).toHaveBeenCalledWith({ force: true });
    });

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "GitHub connection" }),
      ).not.toBeInTheDocument();
    });

    expect(
      await screen.findByText(
        "GitHub browser reconnect completed. 2 repository projects are currently bound to it.",
      ),
    ).toBeInTheDocument();

    const githubCard = screen
      .getByRole("heading", { name: "GitHub" })
      .closest("section");

    expect(githubCard).not.toBeNull();
    expect(
      within(githubCard as HTMLElement).getByRole("button", {
        name: "Review recent reconnect",
      }),
    ).toBeInTheDocument();
    expect(onResult).toHaveBeenCalledTimes(1);
    expect(onResult).toHaveBeenCalledWith(
      expect.objectContaining({
        message:
          "GitHub browser reconnect completed. 2 repository projects are currently bound to it.",
        outcome: "reconnected",
      }),
    );
  });

  it("dismisses the auth overlay without mutating the provider state", async () => {
    render(
      <OverlayProvider>
        <AuthProvidersFocusScreen />
      </OverlayProvider>,
    );

    const trigger = await screen.findByRole("button", {
      name: /Review (connection|reconnect)/i,
    });

    trigger.focus();
    fireEvent.click(trigger);

    const dialog = await screen.findByRole("dialog", {
      name: "GitHub connection",
    });

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Close overlay" }),
    );

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "GitHub connection" }),
      ).not.toBeInTheDocument();
      expect(trigger).toHaveFocus();
    });

    expect(loginWithGithubAuthMock).not.toHaveBeenCalled();
    expect(
      screen.queryByText(/GitHub browser reconnect completed\./i),
    ).not.toBeInTheDocument();
  });

  it("autofocuses the auth overlay primary action and restores focus on Escape", async () => {
    render(
      <OverlayProvider>
        <AuthProvidersFocusScreen />
      </OverlayProvider>,
    );

    const trigger = await screen.findByRole("button", {
      name: /Review (connection|reconnect)/i,
    });

    trigger.focus();
    fireEvent.click(trigger);

    const dialog = await screen.findByRole("dialog", {
      name: "GitHub connection",
    });
    const continueButton = within(dialog).getByRole("button", {
      name: "Continue",
    });

    expect(continueButton).toHaveFocus();

    fireEvent.keyDown(dialog, { key: "Escape" });

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "GitHub connection" }),
      ).not.toBeInTheDocument();
      expect(trigger).toHaveFocus();
    });
  });

  it("moves focus to the browser action when the auth flow advances steps", async () => {
    render(
      <OverlayProvider>
        <AuthProvidersFocusScreen />
      </OverlayProvider>,
    );

    fireEvent.click(
      await screen.findByRole("button", {
        name: /Review (connection|reconnect)/i,
      }),
    );

    const dialog = await screen.findByRole("dialog", {
      name: "GitHub connection",
    });

    fireEvent.click(within(dialog).getByRole("button", { name: "Continue" }));

    await waitFor(() => {
      expect(
        within(dialog).getByRole("button", { name: "Reconnect with browser" }),
      ).toHaveFocus();
    });
  });

  it("keeps the auth overlay open after a failure and allows a retry to recover", async () => {
    loginWithGithubAuthMock.mockReset();
    loginWithGithubAuthMock
      .mockRejectedValueOnce(new Error("Auth offline"))
      .mockResolvedValueOnce(buildAuthProviderStatus());

    render(
      <OverlayProvider>
        <AuthProvidersFocusScreen />
      </OverlayProvider>,
    );

    fireEvent.click(
      await screen.findByRole("button", {
        name: /Review (connection|reconnect)/i,
      }),
    );

    const dialog = await screen.findByRole("dialog", {
      name: "GitHub connection",
    });

    fireEvent.click(within(dialog).getByRole("button", { name: "Continue" }));
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Reconnect with browser" }),
    );

    expect(await within(dialog).findByText("Auth offline")).toBeInTheDocument();
    expect(
      within(dialog).getByRole("button", {
        name: "Retry reconnect with browser",
      }),
    ).toBeInTheDocument();

    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "Retry reconnect with browser",
      }),
    );

    await waitFor(() => {
      expect(loginWithGithubAuthMock).toHaveBeenCalledTimes(2);
    });

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "GitHub connection" }),
      ).not.toBeInTheDocument();
    });

    expect(
      await screen.findByText(
        "GitHub browser reconnect completed. 2 repository projects are currently bound to it.",
      ),
    ).toBeInTheDocument();
  });
});

function buildAuthProviderStatus(
  overrides: Partial<ReturnType<typeof buildAuthProviderStatusShape>> = {},
) {
  return {
    ...buildAuthProviderStatusShape(),
    ...overrides,
  };
}

function buildAuthProviderStatusShape() {
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

function buildSecretSettings() {
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
  };
}
