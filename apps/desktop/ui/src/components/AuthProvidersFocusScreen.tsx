import { startTransition, useEffect, useEffectEvent, useState } from "react";

import { Button } from "./Button";
import AuthProviderConnectionModal from "./AuthProviderConnectionModal";
import {
  type AuthProviderConnectionResult,
  buildAuthProviderActionLabel,
  buildAuthProviderLifecycleSnapshot,
  formatAuthProviderStatus,
  formatBoundRepositoryCount,
  resolveAuthProviderTone,
} from "./authProviderPresentation";
import {
  Badge,
  FocusPageFrame,
  MetaItem,
  MetaRow,
  SurfacePanel,
} from "./Surface";
import { useOverlay } from "./OverlayManager";
import { loadAuthProviders, type AuthProviderStatus } from "../services/auth";

type AuthProvidersFocusScreenProps = {};

export function AuthProvidersFocusScreen({}: AuthProvidersFocusScreenProps) {
  const { openOverlay } = useOverlay();
  const [providers, setProviders] = useState<AuthProviderStatus[]>([]);
  const [lastConnectionResults, setLastConnectionResults] = useState<
    Record<string, AuthProviderConnectionResult>
  >({});
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);

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

  const handleOpenConnectionFlow = useEffectEvent(
    async (provider: AuthProviderStatus) => {
      setActionMessage(null);

      const connectionResult = await openOverlay<AuthProviderConnectionResult>(
        AuthProviderConnectionModal,
        {
          provider,
        },
      );

      if (!connectionResult) {
        return;
      }

      startTransition(() => {
        setProviders((current) =>
          mergeAuthProviderInventory(current, connectionResult.provider),
        );
        setLastConnectionResults((current) => ({
          ...current,
          [connectionResult.provider.provider_id]: connectionResult,
        }));
        setError(null);
        setActionMessage(connectionResult.message);
      });
    },
  );

  const connectedProviderCount = providers.filter(
    (provider) => provider.status === "connected",
  ).length;
  const totalBoundRepositoryCount = providers.reduce(
    (total, provider) => total + provider.bound_repository_count,
    0,
  );

  return (
    <div className="auth-providers-shell">
      <FocusPageFrame
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
        summary={
          <MetaRow>
            <MetaItem label="Providers">
              {isLoading ? "Loading..." : providers.length}
            </MetaItem>
            {!isLoading ? (
              <MetaItem label="Connected">{connectedProviderCount}</MetaItem>
            ) : null}
            {!isLoading ? (
              <MetaItem label="Connected projects">
                {totalBoundRepositoryCount}
              </MetaItem>
            ) : null}
          </MetaRow>
        }
        title="Login Providers"
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

        <SurfacePanel
          className="auth-provider-section"
          description="Host-backed login providers available to the desktop shell. Open the guided connection overlay only when one provider needs to be created, rebound, or refreshed."
          eyebrow="Provider Inventory"
          headerSeparated
          title="Available Accounts"
          tone="section"
        >
          {providers.length > 0 ? (
            <div className="auth-provider-grid">
              {providers.map((provider) => {
                const lifecycleSnapshot = buildAuthProviderLifecycleSnapshot(
                  provider,
                  lastConnectionResults[provider.provider_id],
                );

                return (
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

                    <MetaRow className="auth-provider-card__summary">
                      <MetaItem label="Credential">
                        {provider.credential_name || "No reusable credential"}
                      </MetaItem>
                      <MetaItem label="Usage">
                        {formatBoundRepositoryCount(
                          provider.bound_repository_count,
                        )}
                      </MetaItem>
                    </MetaRow>

                    <MetaRow className="auth-provider-card__summary">
                      <MetaItem label="Stored">
                        {lifecycleSnapshot.storedAtLabel}
                      </MetaItem>
                      <MetaItem label="Refreshed">
                        {lifecycleSnapshot.refreshedAtLabel}
                      </MetaItem>
                    </MetaRow>

                    <MetaRow className="auth-provider-card__summary">
                      <MetaItem label="Lifecycle">
                        {lifecycleSnapshot.lifecycleLabel}
                      </MetaItem>
                      <MetaItem label="Next step">
                        {lifecycleSnapshot.nextActionLabel}
                      </MetaItem>
                    </MetaRow>

                    <div className="auth-provider-card__actions">
                      <Button
                        onClick={() => {
                          void handleOpenConnectionFlow(provider);
                        }}
                        size="sm"
                        variant={
                          provider.status === "connected"
                            ? "secondary"
                            : "primary"
                        }
                      >
                        {buildAuthProviderActionLabel(
                          provider,
                          lastConnectionResults[provider.provider_id],
                        )}
                      </Button>
                    </div>
                  </section>
                );
              })}
            </div>
          ) : null}
        </SurfacePanel>
      </FocusPageFrame>
    </div>
  );
}

function mergeAuthProviderInventory(
  currentProviders: AuthProviderStatus[],
  nextProvider: AuthProviderStatus,
) {
  const providerIndex = currentProviders.findIndex(
    (provider) => provider.provider_id === nextProvider.provider_id,
  );

  if (providerIndex === -1) {
    return [nextProvider, ...currentProviders];
  }

  return currentProviders.map((provider) =>
    provider.provider_id === nextProvider.provider_id ? nextProvider : provider,
  );
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
