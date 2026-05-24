import {
  startTransition,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";

import { Button } from "./Button";
import { SelectField } from "./Field";
import ScreenScaffold from "./ScreenScaffold";
import { Badge, MetaItem, MetaRow, SurfacePanel } from "./Surface";
import { useOverlay } from "./OverlayManager";
import CredentialComposerModal from "./forms/CredentialComposerModal";
import {
  emitLocalizationSettingsChanged,
  useLocalization,
} from "../LocalizationProvider";
import {
  loadApplicationVersion,
  loadLocalizationSettings,
  loadRuntimeDirectories,
  loadRuntimeHealth,
  openExternalUrl,
  openHostPath,
  saveLocalizationPreferences,
  type ApplicationVersionInfo,
  type LocalizationSettings as RuntimeLocalizationSettings,
  type RuntimeAutomationMode,
  type RuntimeDirectorySettings,
  type RuntimeHealthReport,
  type RuntimeHealthStatus,
  type SaveLocalizationPreferencesInput,
} from "../services/runtime";
import {
  loadSecretSettings,
  saveSecretCredential,
  type SaveSecretCredentialInput,
  type SecretCredentialKind,
  type SecretCredentialSetting,
  type SecretSettings,
} from "../services/projects";

type SettingsFocusScreenProps = {
  automationMode: RuntimeAutomationMode | null;
  onManageAuthProviders: () => void;
  onOpenProjects: () => void;
  onOpenProjectWorkers: () => void;
};

type SettingsSnapshot = {
  applicationVersion: ApplicationVersionInfo | null;
  applicationVersionError: string | null;
  isLoading: boolean;
  localizationSettings: RuntimeLocalizationSettings | null;
  localizationSettingsError: string | null;
  runtimeDirectories: RuntimeDirectorySettings | null;
  runtimeDirectoriesError: string | null;
  runtimeHealth: RuntimeHealthReport | null;
  runtimeHealthError: string | null;
  secretSettings: SecretSettings | null;
  secretSettingsError: string | null;
};

type StorageEntry = {
  buttonLabel: string;
  description: string;
  kind: "directory" | "file";
  label: string;
  path: string;
};

type EditableSecretCredentialKind = Exclude<
  SecretCredentialKind,
  "git-http-github-host-login"
>;

const EMPTY_SETTINGS_SNAPSHOT: SettingsSnapshot = {
  applicationVersion: null,
  applicationVersionError: null,
  isLoading: true,
  localizationSettings: null,
  localizationSettingsError: null,
  runtimeDirectories: null,
  runtimeDirectoriesError: null,
  runtimeHealth: null,
  runtimeHealthError: null,
  secretSettings: null,
  secretSettingsError: null,
};

const PUBLIC_DOCUMENTATION_SITE_URL =
  "https://indiegabo.github.io/handy-games-publisher/";

export function SettingsFocusScreen({
  automationMode,
  onManageAuthProviders,
  onOpenProjects,
  onOpenProjectWorkers,
}: SettingsFocusScreenProps) {
  const { t } = useLocalization();
  const { openOverlay } = useOverlay();
  const [snapshot, setSnapshot] = useState<SettingsSnapshot>(
    EMPTY_SETTINGS_SNAPSHOT,
  );
  const [actionError, setActionError] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [isOpeningDocumentation, setIsOpeningDocumentation] = useState(false);
  const [isSavingLocalization, setIsSavingLocalization] = useState(false);
  const [pendingPathLabel, setPendingPathLabel] = useState<string | null>(null);
  const latestRequestIdRef = useRef(0);
  const hasLoadedData = Boolean(
    snapshot.applicationVersion ||
    snapshot.localizationSettings ||
    snapshot.runtimeDirectories ||
    snapshot.runtimeHealth ||
    snapshot.secretSettings,
  );

  const refreshSnapshot = useEffectEvent(async () => {
    const requestId = latestRequestIdRef.current + 1;
    latestRequestIdRef.current = requestId;

    startTransition(() => {
      setSnapshot((current) => ({
        ...current,
        isLoading: true,
      }));
    });

    const [
      applicationVersionResult,
      localizationSettingsResult,
      runtimeHealthResult,
      runtimeDirectoriesResult,
      secretSettingsResult,
    ] = await Promise.allSettled([
      loadApplicationVersion(),
      loadLocalizationSettings(),
      loadRuntimeHealth(),
      loadRuntimeDirectories(),
      loadSecretSettings(),
    ]);

    if (requestId !== latestRequestIdRef.current) {
      return;
    }

    startTransition(() => {
      setSnapshot((current) => ({
        applicationVersion:
          applicationVersionResult.status === "fulfilled"
            ? applicationVersionResult.value
            : current.applicationVersion,
        applicationVersionError:
          applicationVersionResult.status === "fulfilled"
            ? null
            : buildErrorMessage(applicationVersionResult.reason),
        isLoading: false,
        localizationSettings:
          localizationSettingsResult.status === "fulfilled"
            ? localizationSettingsResult.value
            : current.localizationSettings,
        localizationSettingsError:
          localizationSettingsResult.status === "fulfilled"
            ? null
            : buildErrorMessage(localizationSettingsResult.reason),
        runtimeDirectories:
          runtimeDirectoriesResult.status === "fulfilled"
            ? runtimeDirectoriesResult.value
            : current.runtimeDirectories,
        runtimeDirectoriesError:
          runtimeDirectoriesResult.status === "fulfilled"
            ? null
            : buildErrorMessage(runtimeDirectoriesResult.reason),
        runtimeHealth:
          runtimeHealthResult.status === "fulfilled"
            ? runtimeHealthResult.value
            : current.runtimeHealth,
        runtimeHealthError:
          runtimeHealthResult.status === "fulfilled"
            ? null
            : buildErrorMessage(runtimeHealthResult.reason),
        secretSettings:
          secretSettingsResult.status === "fulfilled"
            ? secretSettingsResult.value
            : current.secretSettings,
        secretSettingsError:
          secretSettingsResult.status === "fulfilled"
            ? null
            : buildErrorMessage(secretSettingsResult.reason),
      }));
    });
  });

  useEffect(() => {
    void refreshSnapshot();
  }, []);

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
        setActionError(null);
        setActionMessage(
          scope === "publish"
            ? "Reusable Itch credential saved. It can now be selected from publish destinations."
            : "Reusable repository credential saved. It can now be selected from project repository access settings.",
        );
      });

      await refreshSnapshot();
    },
  );

  const handleSaveLocalizationPreferences = useEffectEvent(
    async (input: SaveLocalizationPreferencesInput) => {
      startTransition(() => {
        setActionError(null);
        setActionMessage(null);
        setIsSavingLocalization(true);
      });

      try {
        const localizationSettings = await saveLocalizationPreferences(input);
        emitLocalizationSettingsChanged(localizationSettings);
        startTransition(() => {
          setSnapshot((current) => ({
            ...current,
            localizationSettings,
            localizationSettingsError: null,
          }));
          setActionMessage(
            "Language preferences saved. The shell will use the updated primary and fallback locale selection as translated surfaces adopt the locale contract.",
          );
        });
      } catch (error) {
        startTransition(() => {
          setActionError(
            `Could not save language preferences: ${buildErrorMessage(error)}`,
          );
        });
      } finally {
        startTransition(() => {
          setIsSavingLocalization(false);
        });
      }
    },
  );

  const handlePrimaryLocaleChange = useEffectEvent(
    async (primaryLocale: string) => {
      if (!snapshot.localizationSettings) {
        return;
      }

      await handleSaveLocalizationPreferences({
        fallback_locale: snapshot.localizationSettings.fallback_locale,
        primary_locale: primaryLocale,
      });
    },
  );

  const handleFallbackLocaleChange = useEffectEvent(
    async (fallbackLocale: string) => {
      if (!snapshot.localizationSettings) {
        return;
      }

      await handleSaveLocalizationPreferences({
        fallback_locale: fallbackLocale,
        primary_locale: snapshot.localizationSettings.primary_locale,
      });
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
        setActionError(null);
        setActionMessage(
          scope === "publish"
            ? "Reusable Itch credential updated. Publish destinations will use the refreshed secret on the next run."
            : "Reusable repository credential updated. Connected project access will use the refreshed secret on the next check.",
        );
      });

      await refreshSnapshot();
    },
  );

  const handleOpenHostPath = useEffectEvent(async (entry: StorageEntry) => {
    startTransition(() => {
      setActionError(null);
      setActionMessage(null);
      setPendingPathLabel(entry.label);
    });

    try {
      await openHostPath(entry.path);
    } catch (error) {
      startTransition(() => {
        setActionError(
          `Could not open ${entry.label.toLowerCase()}: ${buildErrorMessage(error)}`,
        );
      });
    } finally {
      startTransition(() => {
        setPendingPathLabel(null);
      });
    }
  });

  const handleOpenDocumentation = useEffectEvent(async () => {
    startTransition(() => {
      setActionError(null);
      setActionMessage(null);
      setIsOpeningDocumentation(true);
    });

    try {
      await openExternalUrl(PUBLIC_DOCUMENTATION_SITE_URL);
    } catch (error) {
      startTransition(() => {
        setActionError(
          `Could not open beta documentation: ${buildErrorMessage(error)}`,
        );
      });
    } finally {
      startTransition(() => {
        setIsOpeningDocumentation(false);
      });
    }
  });

  const runtimeStatus = snapshot.runtimeHealth?.status ?? null;
  const credentialCount = snapshot.secretSettings?.credentials.length ?? 0;
  const localizationWarningCount =
    snapshot.localizationSettings?.warnings.length ?? 0;
  const readyCredentialCount =
    snapshot.secretSettings?.credentials.filter(
      (credential) => credential.config_summary.status === "ready",
    ).length ?? 0;
  const warningCount = snapshot.secretSettings?.warnings.length ?? 0;
  const localizationOptions = buildLocalizationOptions(
    snapshot.localizationSettings,
  );
  const localizationRootEntry = snapshot.localizationSettings
    ? {
        buttonLabel: "Open localization root",
        description:
          "Shell-discovered JSON locale packs stored alongside the desktop application.",
        kind: "directory" as const,
        label: "Localization root",
        path: snapshot.localizationSettings.localization_root,
      }
    : null;
  const storageEntries = snapshot.runtimeDirectories
    ? buildStorageEntries(snapshot.runtimeDirectories)
    : [];

  return (
    <div className="settings-focus-shell">
      <ScreenScaffold
        actions={
          <Button
            disabled={snapshot.isLoading}
            leadingIcon="refresh"
            onClick={() => void refreshSnapshot()}
            size="sm"
            variant="secondary"
          >
            {snapshot.isLoading
              ? hasLoadedData
                ? "Refreshing..."
                : "Loading..."
              : "Refresh snapshot"}
          </Button>
        }
        eyebrow="Shell"
        subtitle="Inspect the packaged shell, audit shared credentials, and open runtime storage without leaving the operator workflow."
        summary={
          <MetaRow>
            <MetaItem label="Shell">
              {snapshot.applicationVersion?.app_version ?? "loading..."}
            </MetaItem>
            <MetaItem label="Runtime">
              {formatRuntimeStatus(runtimeStatus)}
            </MetaItem>
            <MetaItem label="Automation">
              {formatAutomationMode(automationMode)}
            </MetaItem>
            <MetaItem label="Credentials">{credentialCount}</MetaItem>
          </MetaRow>
        }
        title="Settings"
      >
        {actionMessage ? (
          <p className="notice-banner">{actionMessage}</p>
        ) : null}
        {actionError ? (
          <p className="feed-banner feed-banner--error">{actionError}</p>
        ) : null}

        <SurfacePanel
          actions={
            <div className="settings-focus-action-row">
              <Button
                leadingIcon="layout"
                onClick={onOpenProjectWorkers}
                size="sm"
                variant="secondary"
              >
                Open project workers
              </Button>
            </div>
          }
          description="Use one stable place to confirm what build of the shell is running and whether the local automation host is actually healthy."
          eyebrow="Release"
          headerSeparated
          summary={
            <MetaRow>
              <MetaItem label="Product">
                {snapshot.applicationVersion?.product_name ?? "loading..."}
              </MetaItem>
              <MetaItem label="Shell version">
                {snapshot.applicationVersion?.app_version ?? "loading..."}
              </MetaItem>
              <MetaItem label="Runtime version">
                {snapshot.runtimeHealth?.runtime_version ?? "loading..."}
              </MetaItem>
              <MetaItem label="Platform">
                {snapshot.runtimeHealth?.platform ?? "loading..."}
              </MetaItem>
            </MetaRow>
          }
          title="Shell And Runtime"
        >
          {snapshot.runtimeHealthError ? (
            <p className="feed-banner feed-banner--error">
              {snapshot.runtimeHealthError}
            </p>
          ) : null}
          {snapshot.applicationVersionError ? (
            <p className="feed-banner feed-banner--error">
              {snapshot.applicationVersionError}
            </p>
          ) : null}

          {snapshot.runtimeHealth || snapshot.applicationVersion ? (
            <div className="settings-focus-panel-stack">
              <div className="settings-focus-status-row">
                <Badge tone={resolveRuntimeBadgeTone(runtimeStatus)}>
                  {formatRuntimeStatus(runtimeStatus)}
                </Badge>
                <p className="settings-focus-copy">
                  {buildRuntimeCopy(runtimeStatus, automationMode)}
                </p>
              </div>

              <MetaRow>
                <MetaItem label="Runtime name">
                  {snapshot.runtimeHealth?.runtime_name ?? "loading..."}
                </MetaItem>
                <MetaItem label="Log level">
                  {snapshot.runtimeHealth?.log_level ?? "loading..."}
                </MetaItem>
                <MetaItem label="Process id">
                  {snapshot.runtimeHealth?.process_id ?? "loading..."}
                </MetaItem>
                <MetaItem label="Health source">
                  {snapshot.runtimeHealth?.health_report_path ?? "loading..."}
                </MetaItem>
              </MetaRow>
            </div>
          ) : (
            <p className="settings-focus-copy">
              Loading the latest shell and runtime snapshot...
            </p>
          )}
        </SurfacePanel>

        <SurfacePanel
          actions={
            <div className="settings-focus-action-row">
              <Button
                disabled={
                  pendingPathLabel !== null || localizationRootEntry === null
                }
                leadingIcon="folder"
                onClick={() => {
                  if (!localizationRootEntry) {
                    return;
                  }

                  void handleOpenHostPath(localizationRootEntry);
                }}
                size="sm"
                variant="secondary"
              >
                {pendingPathLabel === "Localization root"
                  ? "Opening..."
                  : "Open localization root"}
              </Button>
            </div>
          }
          description="Choose the primary and fallback locale contract, then inspect the shell-owned folder where JSON locale packs are discovered."
          eyebrow="Localization"
          headerSeparated
          summary={
            <MetaRow>
              <MetaItem label="Primary">
                {snapshot.localizationSettings?.primary_locale ?? "loading..."}
              </MetaItem>
              <MetaItem label="Fallback">
                {snapshot.localizationSettings?.fallback_locale ?? "loading..."}
              </MetaItem>
              <MetaItem label="Available">
                {snapshot.localizationSettings?.available_locales.length ?? 0}
              </MetaItem>
              <MetaItem label="Warnings">{localizationWarningCount}</MetaItem>
            </MetaRow>
          }
          title={t("settings.localization.title", "Language And Localization")}
        >
          {snapshot.localizationSettingsError ? (
            <p className="feed-banner feed-banner--error">
              {snapshot.localizationSettingsError}
            </p>
          ) : null}

          {snapshot.localizationSettings ? (
            <div className="settings-focus-panel-stack">
              {snapshot.localizationSettings.warnings.length > 0 ? (
                <div className="settings-focus-warning-list">
                  {snapshot.localizationSettings.warnings.map((warning) => (
                    <p className="feed-banner" key={warning}>
                      {warning}
                    </p>
                  ))}
                </div>
              ) : null}

              <div className="settings-focus-form-grid">
                <SelectField
                  disabled={isSavingLocalization}
                  hint="Discovered from the shell localization root."
                  label={t(
                    "settings.localization.primary_label",
                    "Primary language",
                  )}
                  onChange={(event) =>
                    void handlePrimaryLocaleChange(event.currentTarget.value)
                  }
                  options={localizationOptions}
                  value={snapshot.localizationSettings.primary_locale}
                />
                <SelectField
                  disabled={isSavingLocalization}
                  hint="Used when a translated string is missing from the primary pack."
                  label={t(
                    "settings.localization.fallback_label",
                    "Fallback language",
                  )}
                  onChange={(event) =>
                    void handleFallbackLocaleChange(event.currentTarget.value)
                  }
                  options={localizationOptions}
                  value={snapshot.localizationSettings.fallback_locale}
                />
              </div>

              <p className="settings-focus-copy">
                Drop additional JSON locale packs into the localization root.
                Invalid packs stay ignored and are surfaced here as warnings.
              </p>
              <p className="settings-focus-path-item__path">
                {snapshot.localizationSettings.localization_root}
              </p>
            </div>
          ) : (
            <p className="settings-focus-copy">
              Loading the latest localization settings...
            </p>
          )}
        </SurfacePanel>

        <SurfacePanel
          actions={
            <div className="settings-focus-action-row">
              <Button
                disabled={isOpeningDocumentation}
                leadingIcon="arrowUpRight"
                onClick={() => {
                  void handleOpenDocumentation();
                }}
                size="sm"
                variant="secondary"
              >
                {isOpeningDocumentation ? "Opening..." : "Open beta docs"}
              </Button>
            </div>
          }
          description="Reach the operator guide that teaches only the workflows currently shipped in the beta build."
          eyebrow="Documentation"
          headerSeparated
          summary={
            <MetaRow>
              <MetaItem label="Audience">Operators</MetaItem>
              <MetaItem label="Surface">Public web</MetaItem>
              <MetaItem label="Coverage">Beta workflows</MetaItem>
            </MetaRow>
          }
          title="Operator Guides"
        >
          <div className="settings-focus-panel-stack">
            <p className="settings-focus-copy">
              The public guide covers Windows installation, repository and local
              workspace setup, reusable credentials, publish destinations, and
              common recovery paths without sending operators back into the
              repository tree.
            </p>
            <p className="settings-focus-path-item__path">
              {PUBLIC_DOCUMENTATION_SITE_URL}
            </p>
          </div>
        </SurfacePanel>

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
              <Button
                leadingIcon="key"
                onClick={onManageAuthProviders}
                size="sm"
                variant="secondary"
              >
                Manage auth providers
              </Button>
              <Button
                leadingIcon="search"
                onClick={onOpenProjects}
                size="sm"
                variant="secondary"
              >
                Open projects
              </Button>
            </div>
          }
          description="Credential reuse needs one operator-owned inventory so Git host logins and publish secrets can be audited before a release fails in the queue."
          eyebrow="Credentials"
          headerSeparated
          summary={
            <MetaRow>
              <MetaItem label="Stored">{credentialCount}</MetaItem>
              <MetaItem label="Ready">{readyCredentialCount}</MetaItem>
              <MetaItem label="Warnings">{warningCount}</MetaItem>
              <MetaItem label="Kinds">
                {snapshot.secretSettings?.supported_credential_kinds.length ??
                  0}
              </MetaItem>
            </MetaRow>
          }
          title="Credential Inventory"
        >
          {snapshot.secretSettingsError ? (
            <p className="feed-banner feed-banner--error">
              {snapshot.secretSettingsError}
            </p>
          ) : null}

          {snapshot.secretSettings ? (
            <div className="settings-focus-panel-stack">
              {snapshot.secretSettings.warnings.length > 0 ? (
                <div className="settings-focus-warning-list">
                  {snapshot.secretSettings.warnings.map((warning) => (
                    <p className="feed-banner" key={warning}>
                      {warning}
                    </p>
                  ))}
                </div>
              ) : null}

              {snapshot.secretSettings.credentials.length > 0 ? (
                <div className="settings-focus-entry-list">
                  {snapshot.secretSettings.credentials.map((credential) => (
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
                            {credential.config_summary.status.replace(
                              /_/g,
                              " ",
                            )}
                          </Badge>
                        </div>
                      </div>

                      <p className="settings-focus-entry__copy">
                        {credential.config_summary.message}
                      </p>
                    </article>
                  ))}
                </div>
              ) : (
                <p className="settings-focus-copy">
                  No shared credentials are stored yet. Connect a Git host login
                  or save publish credentials from a project before queueing the
                  next release.
                </p>
              )}
            </div>
          ) : (
            <p className="settings-focus-copy">
              Loading credential inventory...
            </p>
          )}
        </SurfacePanel>

        <SurfacePanel
          description="Open the runtime-owned directories and files the operator actually needs when checking logs, retained artifacts, or local state."
          eyebrow="Storage"
          headerSeparated
          summary={
            <MetaRow>
              <MetaItem label="Data root">
                {snapshot.runtimeDirectories?.data_dir ?? "loading..."}
              </MetaItem>
              <MetaItem label="Logs">
                {snapshot.runtimeDirectories?.logs_dir ?? "loading..."}
              </MetaItem>
              <MetaItem label="Artifacts">
                {snapshot.runtimeDirectories?.artifacts_dir ?? "loading..."}
              </MetaItem>
              <MetaItem label="Runs">
                {snapshot.runtimeDirectories?.runs_dir ?? "loading..."}
              </MetaItem>
            </MetaRow>
          }
          title="Runtime Storage"
        >
          {snapshot.runtimeDirectoriesError ? (
            <p className="feed-banner feed-banner--error">
              {snapshot.runtimeDirectoriesError}
            </p>
          ) : null}

          {storageEntries.length > 0 ? (
            <div className="settings-focus-path-list">
              {storageEntries.map((entry) => {
                const isOpening = pendingPathLabel === entry.label;

                return (
                  <article
                    className="settings-focus-path-item"
                    key={entry.label}
                  >
                    <div className="settings-focus-path-item__header">
                      <div>
                        <p className="settings-focus-path-item__title">
                          {entry.label}
                        </p>
                        <p className="settings-focus-path-item__copy">
                          {entry.description}
                        </p>
                      </div>
                      <Button
                        disabled={pendingPathLabel !== null}
                        leadingIcon={
                          entry.kind === "directory" ? "folder" : "arrowUpRight"
                        }
                        onClick={() => void handleOpenHostPath(entry)}
                        size="sm"
                        variant="secondary"
                      >
                        {isOpening ? "Opening..." : entry.buttonLabel}
                      </Button>
                    </div>
                    <p className="settings-focus-path-item__path">
                      {entry.path}
                    </p>
                  </article>
                );
              })}
            </div>
          ) : (
            <p className="settings-focus-copy">
              Loading runtime storage locations...
            </p>
          )}
        </SurfacePanel>
      </ScreenScaffold>
    </div>
  );
}

function buildStorageEntries(
  runtimeDirectories: RuntimeDirectorySettings,
): StorageEntry[] {
  return [
    {
      buttonLabel: "Open data directory",
      description: "Root directory for shell-owned runtime state.",
      kind: "directory",
      label: "Data directory",
      path: runtimeDirectories.data_dir,
    },
    {
      buttonLabel: "Open logs directory",
      description: "Runtime log files and retained diagnostics.",
      kind: "directory",
      label: "Logs directory",
      path: runtimeDirectories.logs_dir,
    },
    {
      buttonLabel: "Open artifacts directory",
      description: "Registered build outputs and exported release artifacts.",
      kind: "directory",
      label: "Artifacts directory",
      path: runtimeDirectories.artifacts_dir,
    },
    {
      buttonLabel: "Open runs directory",
      description: "Retained execution workspaces for build and publish runs.",
      kind: "directory",
      label: "Runs directory",
      path: runtimeDirectories.runs_dir,
    },
    {
      buttonLabel: "Open database file",
      description:
        "SQLite source of truth for repositories, runs, and queue state.",
      kind: "file",
      label: "Database file",
      path: runtimeDirectories.database_path,
    },
    {
      buttonLabel: "Open health report",
      description:
        "Latest runtime health snapshot written for shell inspection.",
      kind: "file",
      label: "Health report",
      path: runtimeDirectories.health_report_path,
    },
    {
      buttonLabel: "Open runtime log",
      description: "Primary host-local runtime log file.",
      kind: "file",
      label: "Runtime log",
      path: runtimeDirectories.runtime_log_path,
    },
  ];
}

function buildLocalizationOptions(
  localizationSettings: RuntimeLocalizationSettings | null,
) {
  if (!localizationSettings) {
    return [];
  }

  return localizationSettings.available_locales.map((locale) => ({
    label: `${locale.native_name} (${locale.code})`,
    title: locale.display_name,
    value: locale.code,
  }));
}

function resolveRuntimeBadgeTone(runtimeStatus: RuntimeHealthStatus | null) {
  if (runtimeStatus === "healthy") {
    return "strong";
  }

  if (runtimeStatus === "unhealthy") {
    return "neutral";
  }

  return "muted";
}

function formatRuntimeStatus(runtimeStatus: RuntimeHealthStatus | null) {
  if (!runtimeStatus) {
    return "health unavailable";
  }

  return runtimeStatus.replace(/_/g, " ");
}

function formatAutomationMode(automationMode: RuntimeAutomationMode | null) {
  if (!automationMode) {
    return "status unavailable";
  }

  return automationMode === "idle" ? "paused" : "active";
}

function buildRuntimeCopy(
  runtimeStatus: RuntimeHealthStatus | null,
  automationMode: RuntimeAutomationMode | null,
) {
  if (!runtimeStatus) {
    return "The shell is still resolving the latest runtime health snapshot.";
  }

  if (runtimeStatus === "healthy" && automationMode === "idle") {
    return "The runtime is online, but automatic polling is paused. Manual checks remain available from the project workers screen.";
  }

  if (runtimeStatus === "healthy") {
    return "The runtime is serving the local automation host normally.";
  }

  if (runtimeStatus === "unhealthy") {
    return "The runtime reported an unhealthy orchestration loop and needs attention before the next release is queued.";
  }

  if (runtimeStatus === "stopped") {
    return "The automation host is offline until the runtime is started again.";
  }

  return "The runtime is transitioning between lifecycle states.";
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

function buildErrorMessage(error: unknown) {
  if (error instanceof Error && error.message.trim()) {
    return error.message.trim();
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return "Unknown shell error.";
}
