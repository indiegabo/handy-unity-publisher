import {
  startTransition,
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { Button, IconButton } from "./Button";
import { SelectField, TextField, type SelectOption } from "./Field";
import ScreenScaffold from "./ScreenScaffold";
import { useLocalization, type Translate } from "../LocalizationProvider";
import {
  dispatchOnDemandReleaseProcess,
  readProjectSettingsVersion,
  type OnDemandReleaseVersionSource,
  type RepositoryInspectionEntry,
} from "../services/projects";
import {
  buildProjectSourceDisplay,
  isLocalWorkspaceSource,
} from "../projectSourcePresentation";

// ─── Types ───────────────────────────────────────────────────────────────────

type ReleaseDraft = {
  releaseVersion: string;
  versionSource: OnDemandReleaseVersionSource;
};

type ReleaseValidationErrors = {
  releaseVersion?: string;
};

// Represents the async state for version detection from project settings.
type DetectedVersion =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; value: string }
  | { status: "error"; message: string };

// Internal navigation phases for the two-step flow.
// "select" shows the project picker; "configure" shows the release form.
type Phase =
  | { kind: "select" }
  | { kind: "configure"; repository: RepositoryInspectionEntry };

export type StartReleaseFocusScreenProps = {
  /** Pre-loaded repository list from the shell worker snapshot. */
  repositories: RepositoryInspectionEntry[];
  /** Called after the shell back button is pressed or Cancel is triggered. */
  onBack: () => void;
  /** Called after a release is successfully queued. */
  onQueued: (gitTag: string, repositoryName: string) => void;
  /** Called when the operator chooses to open the project list instead. */
  onOpenProjects: () => void;
};

// ─── Constants ───────────────────────────────────────────────────────────────

// ─── Component ───────────────────────────────────────────────────────────────

export function StartReleaseFocusScreen({
  repositories,
  onBack,
  onQueued,
  onOpenProjects,
}: StartReleaseFocusScreenProps) {
  const [phase, setPhase] = useState<Phase>({ kind: "select" });

  if (phase.kind === "configure") {
    return (
      <ConfigurePhase
        key={phase.repository.repository_id}
        onBack={() => setPhase({ kind: "select" })}
        onCancel={onBack}
        onOpenProjects={onOpenProjects}
        onQueued={onQueued}
        repository={phase.repository}
      />
    );
  }

  return (
    <SelectPhase
      repositories={repositories}
      onSelect={(repository) => setPhase({ kind: "configure", repository })}
    />
  );
}

// ─── Phase: project selection ─────────────────────────────────────────────────

type SelectPhaseProps = {
  repositories: RepositoryInspectionEntry[];
  onSelect: (repository: RepositoryInspectionEntry) => void;
};

function SelectPhase({ repositories, onSelect }: SelectPhaseProps) {
  const { t } = useLocalization();
  const [query, setQuery] = useState("");
  const deferredQuery = useDeferredValue(query);
  const itemRefs = useRef<Array<HTMLButtonElement | null>>([]);

  const filtered = useMemo(() => {
    const normalized = deferredQuery.trim().toLowerCase();
    if (!normalized) {
      return repositories;
    }

    return repositories.filter((repo) => {
      const display = buildProjectSourceDisplay(repo).toLowerCase();
      return (
        repo.repository_name.toLowerCase().includes(normalized) ||
        display.includes(normalized)
      );
    });
  }, [deferredQuery, repositories]);
  const resultCountHint =
    filtered.length === 1
      ? t("start_release.select.results.one", "1 result")
      : t("start_release.select.results.other", "{{count}} results", {
          count: filtered.length,
        });

  return (
    <ScreenScaffold
      eyebrow={t("start_release.eyebrow", "Release")}
      subtitle={t(
        "start_release.select.subtitle",
        "Choose a project to queue a release.",
      )}
      title={t("start_release.select.title", "Start release")}
    >
      <div className="start-release-screen__body">
        <TextField
          autoComplete="off"
          hint={resultCountHint}
          label={t("start_release.select.filter.label", "Filter projects")}
          leadingIcon="search"
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown" && filtered.length > 0) {
              event.preventDefault();
              itemRefs.current[0]?.focus();
            }
          }}
          placeholder={t(
            "start_release.select.filter.placeholder",
            "Search by name or source path",
          )}
          value={query}
        />

        <div
          aria-label={t("start_release.select.list.aria_label", "Project list")}
          className="select-list-modal__list"
          role="list"
        >
          {repositories.length === 0 ? (
            <div className="feed-state start-release-screen__empty">
              <p className="feed-state__title">
                {t(
                  "start_release.select.empty.no_projects.title",
                  "No registered projects.",
                )}
              </p>
              <p className="feed-state__copy">
                {t(
                  "start_release.select.empty.no_projects.copy",
                  "Create a project first, then return here to queue a release.",
                )}
              </p>
            </div>
          ) : filtered.length === 0 ? (
            <div className="feed-state start-release-screen__empty">
              <p className="feed-state__title">
                {t(
                  "start_release.select.empty.no_results.title",
                  "No results matched the filter.",
                )}
              </p>
              <p className="feed-state__copy">
                {t(
                  "start_release.select.empty.no_results.copy",
                  "Try a broader search term or clear the filter.",
                )}
              </p>
            </div>
          ) : (
            filtered.map((repo, index) => (
              <button
                className="select-list-modal__item"
                key={repo.repository_id}
                onClick={() => onSelect(repo)}
                onKeyDown={(event) => {
                  if (event.key === "ArrowDown") {
                    event.preventDefault();
                    itemRefs.current[
                      Math.min(index + 1, filtered.length - 1)
                    ]?.focus();
                    return;
                  }

                  if (event.key === "ArrowUp") {
                    event.preventDefault();

                    if (index === 0) {
                      // Return focus to the filter input.
                      const container =
                        event.currentTarget.closest(".screen-scaffold");
                      const input =
                        container?.querySelector<HTMLElement>("input");
                      input?.focus();
                      return;
                    }

                    itemRefs.current[index - 1]?.focus();
                    return;
                  }

                  if (event.key === "Home") {
                    event.preventDefault();
                    itemRefs.current[0]?.focus();
                    return;
                  }

                  if (event.key === "End") {
                    event.preventDefault();
                    itemRefs.current[filtered.length - 1]?.focus();
                  }
                }}
                ref={(el) => {
                  itemRefs.current[index] = el;
                }}
                type="button"
              >
                <span className="select-list-modal__item-content">
                  <span className="select-list-modal__item-label">
                    {repo.repository_name}
                  </span>
                  <span className="select-list-modal__item-copy">
                    {buildProjectSourceDisplay(repo)}
                  </span>
                </span>
                <span className="select-list-modal__item-action">
                  {t("start_release.select.actions.select", "Select")}
                </span>
              </button>
            ))
          )}
        </div>
      </div>
    </ScreenScaffold>
  );
}

// ─── Phase: release configuration ────────────────────────────────────────────

type ConfigurePhaseProps = {
  repository: RepositoryInspectionEntry;
  onBack: () => void;
  onCancel: () => void;
  onQueued: (gitTag: string, repositoryName: string) => void;
  onOpenProjects: () => void;
};

function ConfigurePhase({
  repository,
  onBack,
  onCancel,
  onQueued,
  onOpenProjects,
}: ConfigurePhaseProps) {
  const { t } = useLocalization();
  const [draft, setDraft] = useState<ReleaseDraft>({
    releaseVersion: "",
    versionSource: "manual",
  });
  const [validationErrors, setValidationErrors] =
    useState<ReleaseValidationErrors>({});
  const [isQueueing, setIsQueueing] = useState(false);
  const [dispatchError, setDispatchError] = useState<string | null>(null);
  const [detectedVersion, setDetectedVersion] = useState<DetectedVersion>({
    status: "idle",
  });
  const isLocalWorkspace = isLocalWorkspaceSource(repository);
  const versionSourceOptions = buildVersionSourceOptions(t);

  // Reads bundleVersion from the local workspace and caches it in state.
  // Safe to call multiple times; each call replaces the previous result.
  const fetchDetectedVersion = useCallback(async () => {
    const path = repository.local_path;
    if (!path) {
      setDetectedVersion({
        status: "error",
        message: t(
          "start_release.configure.detect.error.no_local_path",
          "No local path available for this project.",
        ),
      });
      return;
    }
    setDetectedVersion({ status: "loading" });
    try {
      const version = await readProjectSettingsVersion(path);
      setDetectedVersion({ status: "ready", value: version });
    } catch (error) {
      setDetectedVersion({
        status: "error",
        message: readErrorMessage(t, error),
      });
    }
  }, [repository.local_path]);

  // Trigger detection immediately when the operator switches to project_settings.
  useEffect(() => {
    if (draft.versionSource === "project_settings") {
      void fetchDetectedVersion();
    }
  }, [draft.versionSource, fetchDetectedVersion]);

  const handleQueueRelease = async () => {
    if (!isLocalWorkspace || isQueueing) {
      return;
    }

    const errors = validateReleaseDraft(t, draft);
    if (errors.releaseVersion) {
      setValidationErrors(errors);
      return;
    }

    setIsQueueing(true);
    setDispatchError(null);

    try {
      const release = await dispatchOnDemandReleaseProcess({
        local_path: repository.local_path ?? repository.repo_url,
        release_version:
          draft.versionSource === "manual" ? draft.releaseVersion.trim() : null,
        repository_id: repository.repository_id,
        source_kind: "local_workspace",
        source_ref: null,
        unity_executable_path_override: null,
        version_source: draft.versionSource,
      });

      onQueued(release.git_tag, repository.repository_name);
    } catch (error) {
      startTransition(() => {
        setDispatchError(readErrorMessage(t, error));
        setIsQueueing(false);
      });
    }
  };

  return (
    <ScreenScaffold
      eyebrow={t("start_release.eyebrow", "Release")}
      footer={
        <div className="start-release-screen__footer">
          <Button
            disabled={isQueueing}
            onClick={onCancel}
            size="sm"
            variant="ghost"
          >
            {t("start_release.configure.actions.cancel", "Cancel")}
          </Button>
          {isLocalWorkspace ? (
            <Button
              disabled={isQueueing}
              leadingIcon="arrowUpRight"
              onClick={() => {
                void handleQueueRelease();
              }}
              size="sm"
              variant="secondary"
            >
              {isQueueing
                ? t(
                    "start_release.configure.actions.queueing",
                    "Queueing...",
                  )
                : t(
                    "start_release.configure.actions.queue_local_release",
                    "Queue Local Release",
                  )}
            </Button>
          ) : (
            <Button
              leadingIcon="layout"
              onClick={onOpenProjects}
              size="sm"
              variant="secondary"
            >
              {t(
                "start_release.configure.actions.open_projects",
                "Open projects",
              )}
            </Button>
          )}
        </div>
      }
      subtitle={buildProjectSourceDisplay(repository)}
      title={t(
        "start_release.configure.title",
        "Start release · {{repositoryName}}",
        { repositoryName: repository.repository_name },
      )}
    >
      <div className="start-release-screen__body">
        <button
          className="start-release-screen__back-link"
          onClick={onBack}
          type="button"
        >
          {t(
            "start_release.configure.actions.back",
            "← Back to project list",
          )}
        </button>

        {isLocalWorkspace ? (
          <div className="project-detail-form-grid">
            <SelectField
              hint={t(
                "start_release.configure.version_source.hint",
                "Choose whether HGP should use a manual release label or detect it from project settings.",
              )}
              label={t(
                "start_release.configure.version_source.label",
                "Version source",
              )}
              onChange={(event) => {
                const versionSource = event.currentTarget
                  .value as OnDemandReleaseVersionSource;
                setDraft((current) => ({ ...current, versionSource }));
              }}
              options={versionSourceOptions}
              value={draft.versionSource}
            />
            <div className="start-release-screen__version-row">
              <TextField
                autoFocus
                disabled={draft.versionSource !== "manual"}
                error={validationErrors.releaseVersion}
                hint={resolveVersionHint(t, draft.versionSource, detectedVersion)}
                label={t(
                  "start_release.configure.release_version.label",
                  "Release version",
                )}
                onChange={(event) => {
                  const releaseVersion = event.currentTarget.value;
                  setDraft((current) => ({ ...current, releaseVersion }));
                }}
                placeholder={draft.versionSource !== "manual" ? "" : "v1.2.3"}
                value={
                  draft.versionSource === "project_settings"
                    ? detectedVersion.status === "ready"
                      ? detectedVersion.value
                      : ""
                    : draft.releaseVersion
                }
              />
              {draft.versionSource === "project_settings" ? (
                <IconButton
                  className="start-release-screen__version-reload"
                  disabled={isQueueing || detectedVersion.status === "loading"}
                  icon="refresh"
                  label={t(
                    "start_release.configure.actions.reload_detected_version",
                    "Reload detected version",
                  )}
                  onClick={() => void fetchDetectedVersion()}
                  size="sm"
                  variant="ghost"
                />
              ) : null}
            </div>
            {dispatchError ? (
              <p className="feed-banner feed-banner--error">{dispatchError}</p>
            ) : null}
          </div>
        ) : (
          <div className="feed-state">
            <p className="feed-state__title">
              {t(
                "start_release.configure.unavailable.title",
                "Quick release is available for local workspace projects.",
              )}
            </p>
            <p className="feed-state__copy">
              {t(
                "start_release.configure.unavailable.copy",
                "Open the project list to queue managed repository releases from the project detail screen.",
              )}
            </p>
          </div>
        )}
      </div>
    </ScreenScaffold>
  );
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

function validateReleaseDraft(
  t: Translate,
  draft: ReleaseDraft,
): ReleaseValidationErrors {
  if (draft.versionSource !== "manual") {
    return {};
  }

  return draft.releaseVersion.trim()
    ? {}
    : {
        releaseVersion: t(
          "start_release.configure.validation.release_version_required",
          "Release version is required for manual local dispatch.",
        ),
      };
}

// Builds the contextual hint string for the Release version field based on the
// current version source selection and the async detection state.
function resolveVersionHint(
  t: Translate,
  versionSource: OnDemandReleaseVersionSource,
  detected: DetectedVersion,
): string {
  if (versionSource === "manual") {
    return t(
      "start_release.configure.release_version.manual_hint",
      "Use the release label that should identify this local snapshot.",
    );
  }
  switch (detected.status) {
    case "idle":
      return t("start_release.configure.release_version.detecting", "Detecting...");
    case "loading":
      return t("start_release.configure.release_version.detecting", "Detecting...");
    case "ready":
      return t(
        "start_release.configure.release_version.detected",
        "Detected from project settings.",
      );
    case "error":
      return t(
        "start_release.configure.release_version.detect_failed",
        "Detection failed: {{message}}",
        { message: detected.message },
      );
  }
}

function readErrorMessage(t: Translate, error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return t(
    "start_release.error.queue_failed",
    "The desktop shell could not queue the release.",
  );
}

function buildVersionSourceOptions(t: Translate): SelectOption[] {
  return [
    {
      label: t(
        "start_release.configure.version_source.manual",
        "Manual version label",
      ),
      value: "manual",
    },
    {
      label: t(
        "start_release.configure.version_source.project_settings",
        "Detect from project settings",
      ),
      value: "project_settings",
    },
  ];
}
