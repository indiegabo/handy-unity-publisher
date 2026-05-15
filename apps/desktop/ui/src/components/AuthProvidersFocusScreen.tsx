import { startTransition, useEffect, useEffectEvent, useState } from "react";

import { Button } from "./Button";
import { Badge, SurfacePanel, type BadgeTone } from "./Surface";
import {
  loadAuthProviders,
  loginWithGithubAuth,
  type AuthProviderStatus,
} from "../services/auth";

type AuthProvidersFocusScreenProps = {};

export function AuthProvidersFocusScreen({}: AuthProvidersFocusScreenProps) {
  const [providers, setProviders] = useState<AuthProviderStatus[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [pendingProviderId, setPendingProviderId] = useState<string | null>(
    null,
  );

  const loadProviders = useEffectEvent(async () => {
    setIsLoading(true);

    try {
      const nextProviders = await loadAuthProviders();
      startTransition(() => {
        setProviders(nextProviders);
        setError(null);
        setIsLoading(false);
      });
    } catch (loadError) {
      startTransition(() => {
        setError(buildAuthProviderErrorMessage(loadError));
        setIsLoading(false);
      });
    }
  });

  useEffect(() => {
    void loadProviders();
  }, []);

  const handleGithubLogin = useEffectEvent(async () => {
    setPendingProviderId("github");
    setActionMessage(null);

    try {
      const provider = await loginWithGithubAuth();
      startTransition(() => {
        setProviders([provider]);
        setError(null);
        setActionMessage(
          `GitHub login connected. ${formatBoundRepositoryCount(
            provider.bound_repository_count,
          )} now use it by default.`,
        );
      });
    } catch (loginError) {
      startTransition(() => {
        setError(buildAuthProviderErrorMessage(loginError));
      });
    } finally {
      startTransition(() => {
        setPendingProviderId(null);
      });
    }
  });

  return (
    <div className="auth-providers-shell">
      <SurfacePanel
        className="focus-primary-panel"
        actions={
          <Button
            leadingIcon="refresh"
            onClick={() => void loadProviders()}
            size="sm"
            variant="secondary"
          >
            Refresh
          </Button>
        }
        description="GitHub login is delegated to Git Credential Manager and reused by repository polling and checkout flows."
        eyebrow="Accounts"
        title="Possible Logins"
      >
        {error ? (
          <p className="feed-banner feed-banner--error">{error}</p>
        ) : null}
        {actionMessage ? (
          <p className="feed-banner feed-banner--success">{actionMessage}</p>
        ) : null}

        {isLoading && providers.length === 0 ? (
          <div className="feed-state">
            <p className="feed-state__title">Loading login providers...</p>
            <p className="feed-state__copy">
              The shell is checking which host-backed authentication providers
              are ready on this machine.
            </p>
          </div>
        ) : null}

        {!isLoading && providers.length === 0 ? (
          <div className="feed-state">
            <p className="feed-state__title">
              No login providers are available.
            </p>
            <p className="feed-state__copy">
              Install the required host tooling to enable repository
              authentication flows.
            </p>
          </div>
        ) : null}

        {providers.length > 0 ? (
          <div className="auth-provider-grid">
            {providers.map((provider) => (
              <section
                className="auth-provider-card"
                key={provider.provider_id}
              >
                <header className="auth-provider-card__header">
                  <div className="auth-provider-card__title-block">
                    <h3 className="auth-provider-card__title">
                      {provider.label}
                    </h3>
                    <p className="auth-provider-card__copy">
                      {provider.instance_url}
                    </p>
                  </div>
                  <Badge tone={resolveAuthProviderTone(provider.status)}>
                    {formatAuthProviderStatus(provider.status)}
                  </Badge>
                </header>

                <p className="auth-provider-card__copy">
                  {provider.status_message}
                </p>

                <div className="auth-provider-card__meta">
                  {provider.credential_name ? (
                    <Badge tone="muted">{provider.credential_name}</Badge>
                  ) : null}
                  <Badge tone="muted">
                    {formatBoundRepositoryCount(
                      provider.bound_repository_count,
                    )}
                  </Badge>
                </div>

                <div className="auth-provider-card__actions">
                  <Button
                    disabled={pendingProviderId === provider.provider_id}
                    leadingIcon="arrowUpRight"
                    onClick={() => void handleGithubLogin()}
                    size="sm"
                    variant={
                      provider.status === "connected" ? "secondary" : "primary"
                    }
                  >
                    {pendingProviderId === provider.provider_id
                      ? "Connecting..."
                      : provider.status === "connected"
                        ? "Reconnect with browser"
                        : "Log in with browser"}
                  </Button>
                </div>
              </section>
            ))}
          </div>
        ) : null}
      </SurfacePanel>
    </div>
  );
}

function resolveAuthProviderTone(status: string): BadgeTone {
  switch (status) {
    case "connected":
      return "strong";
    case "disconnected":
      return "neutral";
    default:
      return "muted";
  }
}

function formatAuthProviderStatus(status: string) {
  switch (status) {
    case "connected":
      return "connected";
    case "disconnected":
      return "ready to connect";
    default:
      return "unavailable";
  }
}

function formatBoundRepositoryCount(boundRepositoryCount: number) {
  return `${boundRepositoryCount} repository project${
    boundRepositoryCount === 1 ? "" : "s"
  }`;
}

function buildAuthProviderErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return "The desktop shell could not resolve the authentication provider state.";
}
