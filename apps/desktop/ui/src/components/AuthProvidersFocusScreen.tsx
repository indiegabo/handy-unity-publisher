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
import { useLocalization } from "../LocalizationProvider";
import { loadAuthProviders, type AuthProviderStatus } from "../services/auth";

type AuthProvidersFocusScreenProps = {
  onResult?: (result: AuthProviderConnectionResult) => void;
};

export function AuthProvidersFocusScreen({
  onResult,
}: AuthProvidersFocusScreenProps) {
  const { openOverlay } = useOverlay();
  const { t } = useLocalization();
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
          setError(buildAuthProviderErrorMessage(t, loadError));
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
          {isRefreshing
            ? t("auth_providers.actions.refreshing", "Refreshing providers...")
            : t("auth_providers.actions.refresh", "Refresh providers")}
        </Button>
      }
      className="auth-providers-shell"
      eyebrow={t("auth_providers.eyebrow", "Accounts")}
      subtitle={t(
        "auth_providers.subtitle",
        "GitHub login is delegated to Git Credential Manager and reused by repository polling and checkout flows.",
      )}
      summary={
        <MetaRow>
          <MetaItem label={t("auth_providers.summary.providers", "Providers")}>
            {showsProviderLoading
              ? t("auth_providers.summary.loading", "Loading...")
              : providers.length}
          </MetaItem>
          {!showsProviderLoading ? (
            <MetaItem
              label={t("auth_providers.summary.connected", "Connected")}
            >
              {connectedProviderCount}
            </MetaItem>
          ) : null}
          {!showsProviderLoading ? (
            <MetaItem
              label={t(
                "auth_providers.summary.connected_projects",
                "Connected projects",
              )}
            >
              {totalBoundRepositoryCount}
            </MetaItem>
          ) : null}
        </MetaRow>
      }
      title={t("auth_providers.title", "Login Providers")}
    >
      {inventoryStale && error ? (
        <>
          <p className="feed-banner feed-banner--error">{error}</p>
          <p className="feed-state__copy">
            {t(
              "auth_providers.stale_copy",
              "Showing the last known provider inventory while the shell retries host-backed authentication discovery.",
            )}
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
          <p className="feed-state__title">
            {t("auth_providers.loading.title", "Loading login providers...")}
          </p>
          <p className="feed-state__copy">
            {t(
              "auth_providers.loading.copy",
              "The shell is checking which host-backed authentication providers are ready on this machine.",
            )}
          </p>
        </div>
      ) : null}

      {showsProviderUnavailable ? (
        <div className="feed-state">
          <p className="feed-state__title">
            {t(
              "auth_providers.unavailable.title",
              "Login provider inventory is unavailable.",
            )}
          </p>
          <p className="feed-state__copy">{error}</p>
          <Button
            leadingIcon="refresh"
            onClick={() => void loadProviders("refresh")}
            size="sm"
            variant="secondary"
          >
            {t("auth_providers.actions.retry", "Retry provider load")}
          </Button>
        </div>
      ) : null}

      {!showsProviderLoading && inventoryAvailable && providers.length === 0 ? (
        <div className="feed-state">
          <p className="feed-state__title">
            {t(
              "auth_providers.empty.title",
              "No login providers are available.",
            )}
          </p>
          <p className="feed-state__copy">
            {t(
              "auth_providers.empty.copy",
              "Install the required host tooling to enable repository authentication flows.",
            )}
          </p>
        </div>
      ) : null}

      {inventoryAvailable && providers.length > 0 ? (
        <SurfacePanel
          className="auth-provider-section"
          description={t(
            "auth_providers.inventory.description",
            "Host-backed login providers available to the desktop shell. Open the guided connection overlay only when one provider needs to be created, rebound, or refreshed.",
          )}
          eyebrow={t("auth_providers.inventory.eyebrow", "Provider Inventory")}
          headerSeparated
          title={t("auth_providers.inventory.title", "Available Accounts")}
          tone="section"
        >
          <div className="auth-provider-grid">
            {providers.map((provider) => {
              const lifecycleSnapshot = buildAuthProviderLifecycleSnapshot(
                t,
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
                      {formatAuthProviderStatus(t, provider.status)}
                    </Badge>
                  </header>

                  <p className="auth-provider-card__copy">
                    {provider.status_message}
                  </p>

                  <SummaryStrip className="auth-provider-card__summary-strip">
                    {buildAuthProviderSummaryRows(
                      t,
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
                        t,
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

function buildAuthProviderErrorMessage(
  t: ReturnType<typeof useLocalization>["t"],
  error: unknown,
): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return t(
    "auth_providers.error.fallback",
    "The desktop shell could not resolve the authentication provider state.",
  );
}
