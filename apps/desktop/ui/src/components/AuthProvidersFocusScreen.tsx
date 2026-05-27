import { startTransition, useEffect, useEffectEvent, useState } from "react";

import { Button } from "./Button";
import AuthProviderConnectionModal from "./AuthProviderConnectionModal";
import {
  type AuthProviderConnectionResult,
  buildAuthProviderActionLabel,
  formatAuthProviderStatus,
  resolveAuthProviderTone,
} from "./authProviderPresentation";
import { Badge, SurfacePanel } from "./Surface";
import { useOverlay } from "./OverlayManager";
import ScreenScaffold from "./ScreenScaffold";
import CredentialComposerModal from "./forms/CredentialComposerModal";
import { useLocalization } from "../LocalizationProvider";
import { loadAuthProviders, type AuthProviderStatus } from "../services/auth";
import {
  loadSecretSettings,
  saveSecretCredential,
  type SaveSecretCredentialInput,
  type SecretCredentialKind,
  type SecretCredentialSetting,
  type SecretSettings,
} from "../services/projects";

type AuthProvidersFocusScreenProps = {
  onResult?: (result: AuthProviderConnectionResult) => void;
};

type EditableSecretCredentialKind = Exclude<
  SecretCredentialKind,
  "git-http-github-host-login"
>;

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
  const [secretSettings, setSecretSettings] = useState<SecretSettings | null>(
    null,
  );
  const [secretSettingsError, setSecretSettingsError] = useState<string | null>(
    null,
  );

  const loadInventory = useEffectEvent(
    async (reason: "initial" | "refresh") => {
      if (reason === "refresh" && inventoryAvailable) {
        setIsRefreshing(true);
        setInventoryStale(false);
        setError(null);
      } else {
        setIsLoading(true);
      }

      const [providerResult, secretSettingsResult] = await Promise.allSettled([
        loadAuthProviders(),
        loadSecretSettings(),
      ]);

      startTransition(() => {
        if (providerResult.status === "fulfilled") {
          setProviders(providerResult.value);
          setInventoryAvailable(true);
          setInventoryStale(false);
          setError(null);
        } else {
          setError(buildAuthProviderErrorMessage(t, providerResult.reason));
          setInventoryStale(inventoryAvailable);
        }

        if (secretSettingsResult.status === "fulfilled") {
          setSecretSettings(secretSettingsResult.value);
          setSecretSettingsError(null);
        } else {
          setSecretSettingsError(
            buildInventoryErrorMessage(secretSettingsResult.reason),
          );
        }

        setIsLoading(false);
        setIsRefreshing(false);
      });
    },
  );

  useEffect(() => {
    void loadInventory("initial");
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

  const handleCreateCredential = useEffectEvent(
    async (scope: "repository" | "publish") => {
      const providerLabel = scope === "publish" ? "Itch.io" : "Git host";
      const created = await openOverlay(CredentialComposerModal, {
        onSubmit: async (input: SaveSecretCredentialInput) => {
          await saveSecretCredential(input);
        },
        providerLabel,
        scope,
      });

      if (!created) {
        return;
      }

      startTransition(() => {
        setActionMessage(
          scope === "publish"
            ? "Reusable Itch credential saved. It can now be selected from publish destinations."
            : "Reusable repository credential saved. It can now be selected from project repository access settings.",
        );
      });

      await loadInventory("refresh");
    },
  );

  const handleEditCredential = useEffectEvent(
    async (credential: SecretCredentialSetting) => {
      const editableKind = toEditableSecretCredentialKind(credential.kind);
      if (!editableKind) {
        return;
      }

      const scope = editableKind === "itch-api-key" ? "publish" : "repository";
      const providerLabel = scope === "publish" ? "Itch.io" : "Git host";
      const updated = await openOverlay(CredentialComposerModal, {
        initialCredential: {
          credentialId: credential.credential_id,
          kind: editableKind,
          name: credential.name,
        },
        onSubmit: async (input: SaveSecretCredentialInput) => {
          await saveSecretCredential(input);
        },
        providerLabel,
        scope,
      });

      if (!updated) {
        return;
      }

      startTransition(() => {
        setActionMessage(
          scope === "publish"
            ? "Reusable Itch credential updated. Publish destinations will use the refreshed secret on the next run."
            : "Reusable repository credential updated. Connected project access will use the refreshed secret on the next check.",
        );
      });

      await loadInventory("refresh");
    },
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
          onClick={() => void loadInventory("refresh")}
          size="sm"
          variant="secondary"
        >
          {isRefreshing
            ? t("auth_providers.actions.refreshing", "Refreshing providers...")
            : t("auth_providers.actions.refresh", "Refresh providers")}
        </Button>
      }
      className="auth-providers-shell"
      title={t("auth_providers.title", "Login Providers")}
    >
      {(inventoryStale && error) ||
      (!inventoryStale && error && inventoryAvailable) ? (
        <p className="feed-banner feed-banner--error">{error}</p>
      ) : null}
      {actionMessage ? (
        <p className="feed-banner feed-banner--success">{actionMessage}</p>
      ) : null}

      {showsProviderLoading ? (
        <p className="settings-focus-copy">
          {t("auth_providers.loading.title", "Loading login providers...")}
        </p>
      ) : null}

      {showsProviderUnavailable ? (
        <div className="settings-focus-action-row">
          <Button
            leadingIcon="refresh"
            onClick={() => void loadInventory("refresh")}
            size="sm"
            variant="secondary"
          >
            {t("auth_providers.actions.retry", "Retry provider load")}
          </Button>
        </div>
      ) : null}

      {!showsProviderLoading && inventoryAvailable && providers.length === 0 ? (
        <p className="settings-focus-copy">
          {t("auth_providers.empty.title", "No login providers are available.")}
        </p>
      ) : null}

      {inventoryAvailable && providers.length > 0 ? (
        <SurfacePanel
          title={t("auth_providers.inventory.title", "Available Accounts")}
        >
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
                    {formatAuthProviderStatus(t, provider.status)}
                  </Badge>
                </header>

                <div className="auth-provider-card__actions">
                  <Button
                    onClick={() => {
                      void handleOpenConnectionFlow(provider);
                    }}
                    size="sm"
                    variant={
                      provider.status === "connected" ? "secondary" : "primary"
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
            ))}
          </div>
        </SurfacePanel>
      ) : null}

      <SurfacePanel
        actions={
          <div className="settings-focus-action-row">
            <Button
              leadingIcon="plus"
              onClick={() => void handleCreateCredential("publish")}
              size="sm"
              variant="primary"
            >
              New Itch credential
            </Button>
            <Button
              leadingIcon="plus"
              onClick={() => void handleCreateCredential("repository")}
              size="sm"
              variant="secondary"
            >
              New repository credential
            </Button>
          </div>
        }
        title="Credential Inventory"
      >
        {secretSettingsError ? (
          <p className="feed-banner feed-banner--error">
            {secretSettingsError}
          </p>
        ) : null}

        {secretSettings ? (
          <div className="settings-focus-panel-stack">
            {secretSettings.credentials.length > 0 ? (
              <div className="settings-focus-entry-list">
                {secretSettings.credentials.map((credential) => (
                  <article
                    className="settings-focus-entry"
                    key={credential.credential_id}
                  >
                    <div className="settings-focus-entry__header">
                      <div>
                        <p className="settings-focus-entry__title">
                          {credential.name}
                        </p>
                        <p className="settings-focus-entry__meta">
                          {credential.kind}
                        </p>
                      </div>
                      <div className="settings-focus-entry__controls">
                        {toEditableSecretCredentialKind(credential.kind) ? (
                          <Button
                            aria-label={`Edit ${credential.name}`}
                            onClick={() =>
                              void handleEditCredential(credential)
                            }
                            size="sm"
                            variant="ghost"
                          >
                            Edit
                          </Button>
                        ) : null}
                        <Badge
                          tone={
                            credential.config_summary.status === "ready"
                              ? "strong"
                              : "muted"
                          }
                        >
                          {credential.config_summary.status.replace(/_/g, " ")}
                        </Badge>
                      </div>
                    </div>
                  </article>
                ))}
              </div>
            ) : (
              <p className="settings-focus-copy">
                No shared credentials are stored yet.
              </p>
            )}
          </div>
        ) : (
          <p className="settings-focus-copy">Loading credential inventory...</p>
        )}
      </SurfacePanel>
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

function toEditableSecretCredentialKind(
  kind: string,
): EditableSecretCredentialKind | null {
  switch (kind) {
    case "git-http-basic":
    case "git-http-bearer":
    case "itch-api-key":
      return kind;
    default:
      return null;
  }
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

function buildInventoryErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim()) {
    return error.message.trim();
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return "The desktop shell could not resolve the shared credential inventory.";
}
