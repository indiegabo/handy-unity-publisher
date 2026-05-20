import { startTransition, useEffect, useEffectEvent, useState } from "react";

import { Button } from "./Button";
import AuthProviderConnectionModal from "./AuthProviderConnectionModal";
import {
  type AuthProviderConnectionResult,
  buildAuthProviderActionLabel,
  buildAuthProviderLifecycleSnapshot,
  buildAuthProviderSummaryRows,
  formatAuthProviderStatus,
  resolveAuthProviderTone,
} from "./authProviderPresentation";
import {
  Badge,
  MetaItem,
  MetaRow,
  SummaryStrip,
  SurfacePanel,
} from "./Surface";
import { useOverlay } from "./OverlayManager";
import ScreenScaffold from "./ScreenScaffold";
import { loadAuthProviders, type AuthProviderStatus } from "../services/auth";

type AuthProvidersFocusScreenProps = {
  onResult?: (result: AuthProviderConnectionResult) => void;
};

export function AuthProvidersFocusScreen({
  onResult,
}: AuthProvidersFocusScreenProps) {
  const { openOverlay } = useOverlay();
  const [providers, setProviders] = useState<AuthProviderStatus[]>([]);
  const [lastConnectionResults, setLastConnectionResults] = useState<
    Record<string, AuthProviderConnectionResult>
  >({});
  const [isLoading, setIsLoading] = useState(true);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [inventoryAvailable, setInventoryAvailable] = useState(false);
  const [inventoryStale, setInventoryStale] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);

  const loadProviders = useEffectEvent(
    async (reason: "initial" | "refresh") => {
      if (reason === "refresh" && inventoryAvailable) {
        setIsRefreshing(true);
        setInventoryStale(false);
        setError(null);
      } else {
        setIsLoading(true);
      }

      try {
        const nextProviders = await loadAuthProviders();
        startTransition(() => {
          setProviders(nextProviders);
          setInventoryAvailable(true);
          setInventoryStale(false);
          setError(null);
          setIsLoading(false);
          setIsRefreshing(false);
        });
      } catch (loadError) {
        startTransition(() => {
          setError(buildAuthProviderErrorMessage(loadError));
          setIsLoading(false);
          setIsRefreshing(false);
          setInventoryStale(inventoryAvailable);
        });
      }
    },
  );

  useEffect(() => {
    void loadProviders("initial");
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

      onResult?.(connectionResult);
    },
  );

  const connectedProviderCount = providers.filter(
    (provider) => provider.status === "connected",
  ).length;
  const totalBoundRepositoryCount = providers.reduce(
    (total, provider) => total + provider.bound_repository_count,
    0,
  );
  const showsProviderLoading = isLoading && !inventoryAvailable;
  const showsProviderUnavailable =
    !isLoading && !inventoryAvailable && error !== null;

  return (
    <ScreenScaffold
      actions={
        <Button
          disabled={isLoading || isRefreshing}
          leadingIcon="refresh"
          onClick={() => void loadProviders("refresh")}
          size="sm"
          variant="secondary"
        >
          {isRefreshing ? "Refreshing providers..." : "Refresh providers"}
        </Button>
      }
      className="auth-providers-shell"
      eyebrow="Accounts"
      subtitle="GitHub login is delegated to Git Credential Manager and reused by repository polling and checkout flows."
      summary={
        <MetaRow>
          <MetaItem label="Providers">
            {showsProviderLoading ? "Loading..." : providers.length}
          </MetaItem>
          {!showsProviderLoading ? (
            <MetaItem label="Connected">{connectedProviderCount}</MetaItem>
          ) : null}
          {!showsProviderLoading ? (
            <MetaItem label="Connected projects">
              {totalBoundRepositoryCount}
            </MetaItem>
          ) : null}
        </MetaRow>
      }
      title="Login Providers"
    >
        {inventoryStale && error ? (
          <>
            <p className="feed-banner feed-banner--error">{error}</p>
            <p className="feed-state__copy">
              Showing the last known provider inventory while the shell retries
              host-backed authentication discovery.
            </p>
          </>
        ) : null}

        {!inventoryStale && error && inventoryAvailable ? (
          <p className="feed-banner feed-banner--error">{error}</p>
        ) : null}
        {actionMessage ? (
          <p className="feed-banner feed-banner--success">{actionMessage}</p>
        ) : null}

        {showsProviderLoading ? (
          <div className="feed-state">
            <p className="feed-state__title">Loading login providers...</p>
            <p className="feed-state__copy">
              The shell is checking which host-backed authentication providers
              are ready on this machine.
            </p>
          </div>
        ) : null}

        {showsProviderUnavailable ? (
          <div className="feed-state">
            <p className="feed-state__title">
              Login provider inventory is unavailable.
            </p>
            <p className="feed-state__copy">{error}</p>
            <Button
              leadingIcon="refresh"
              onClick={() => void loadProviders("refresh")}
              size="sm"
              variant="secondary"
            >
              Retry provider load
            </Button>
          </div>
        ) : null}

        {!showsProviderLoading &&
        inventoryAvailable &&
        providers.length === 0 ? (
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

        {inventoryAvailable && providers.length > 0 ? (
          <SurfacePanel
            className="auth-provider-section"
            description="Host-backed login providers available to the desktop shell. Open the guided connection overlay only when one provider needs to be created, rebound, or refreshed."
            eyebrow="Provider Inventory"
            headerSeparated
            title="Available Accounts"
            tone="section"
          >
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

                    <SummaryStrip className="auth-provider-card__summary-strip">
                      {buildAuthProviderSummaryRows(
                        provider,
                        lifecycleSnapshot,
                      ).map((summaryRow, summaryRowIndex) => (
                        <MetaRow
                          className="auth-provider-card__summary"
                          key={`${provider.provider_id}-summary-${summaryRowIndex}`}
                        >
                          {summaryRow.map((item) => (
                            <MetaItem key={item.label} label={item.label}>
                              {item.value}
                            </MetaItem>
                          ))}
                        </MetaRow>
                      ))}
                    </SummaryStrip>

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
          </SurfacePanel>
        ) : null}
    </ScreenScaffold>
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
