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

const { loadAuthProvidersMock, loginWithGithubAuthMock } = vi.hoisted(() => ({
  loadAuthProvidersMock: vi.fn(),
  loginWithGithubAuthMock: vi.fn(),
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
  loadAuthProvidersMock.mockResolvedValue([buildAuthProviderStatus()]);
  loginWithGithubAuthMock.mockResolvedValue(buildAuthProviderStatus());
});

describe("AuthProvidersFocusScreen", () => {
  it("opens the auth overlay and updates provider state after a successful reconnect", async () => {
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
      within(githubCard as HTMLElement).getByText(
        "Browser reconnect completed in this session",
      ),
    ).toBeInTheDocument();
    expect(
      within(githubCard as HTMLElement).getByText("2026-05-19 00:00 UTC"),
    ).toBeInTheDocument();
    expect(
      within(githubCard as HTMLElement).getByText("2026-05-19 00:12 UTC"),
    ).toBeInTheDocument();
    expect(
      within(githubCard as HTMLElement).getByText(
        "Reuse the host credential until repository access fails again",
      ),
    ).toBeInTheDocument();
    expect(
      within(githubCard as HTMLElement).getByRole("button", {
        name: "Review recent reconnect",
      }),
    ).toBeInTheDocument();
  });

  it("dismisses the auth overlay without mutating the provider state", async () => {
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

    fireEvent.click(
      within(dialog).getByRole("button", { name: "Close overlay" }),
    );

    await waitFor(() => {
      expect(
        screen.queryByRole("dialog", { name: "GitHub connection" }),
      ).not.toBeInTheDocument();
    });

    expect(loginWithGithubAuthMock).not.toHaveBeenCalled();
    expect(
      screen.queryByText(/GitHub browser reconnect completed\./i),
    ).not.toBeInTheDocument();
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
