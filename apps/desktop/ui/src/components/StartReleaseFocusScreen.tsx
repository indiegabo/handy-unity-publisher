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

const VERSION_SOURCE_OPTIONS: SelectOption[] = [
  { label: "Manual version label", value: "manual" },
  { label: "Detect from project settings", value: "project_settings" },
];

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

  return (
    <ScreenScaffold
      eyebrow="Release"
      subtitle="Choose a project to queue a release."
      title="Start release"
    >
      <div className="start-release-screen__body">
        <TextField
          autoComplete="off"
          hint={`${filtered.length} result${filtered.length === 1 ? "" : "s"}`}
          label="Filter projects"
          leadingIcon="search"
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown" && filtered.length > 0) {
              event.preventDefault();
              itemRefs.current[0]?.focus();
            }
          }}
          placeholder="Search by name or source path"
          value={query}
        />

        <div
          aria-label="Project list"
          className="select-list-modal__list"
          role="list"
        >
          {repositories.length === 0 ? (
            <div className="feed-state start-release-screen__empty">
              <p className="feed-state__title">No registered projects.</p>
              <p className="feed-state__copy">
                Create a project first, then return here to queue a release.
              </p>
            </div>
          ) : filtered.length === 0 ? (
            <div className="feed-state start-release-screen__empty">
              <p className="feed-state__title">
                No results matched the filter.
              </p>
              <p className="feed-state__copy">
                Try a broader search term or clear the filter.
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
                <span className="select-list-modal__item-action">Select</span>
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

  // Reads bundleVersion from the local workspace and caches it in state.
  // Safe to call multiple times; each call replaces the previous result.
  const fetchDetectedVersion = useCallback(async () => {
    const path = repository.local_path;
    if (!path) {
      setDetectedVersion({
        status: "error",
        message: "No local path available for this project.",
      });
      return;
    }
    setDetectedVersion({ status: "loading" });
    try {
      const version = await readProjectSettingsVersion(path);
      setDetectedVersion({ status: "ready", value: version });
    } catch (error) {
      setDetectedVersion({ status: "error", message: readErrorMessage(error) });
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

    const errors = validateReleaseDraft(draft);
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
        setDispatchError(readErrorMessage(error));
        setIsQueueing(false);
      });
    }
  };

  return (
    <ScreenScaffold
      eyebrow="Release"
      footer={
        <div className="start-release-screen__footer">
          <Button
            disabled={isQueueing}
            onClick={onCancel}
            size="sm"
            variant="ghost"
          >
            Cancel
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
              {isQueueing ? "Queueing..." : "Queue Local Release"}
            </Button>
          ) : (
            <Button
              leadingIcon="layout"
              onClick={onOpenProjects}
              size="sm"
              variant="secondary"
            >
              Open projects
            </Button>
          )}
        </div>
      }
      subtitle={buildProjectSourceDisplay(repository)}
      title={`Start release · ${repository.repository_name}`}
    >
      <div className="start-release-screen__body">
        <button
          className="start-release-screen__back-link"
          onClick={onBack}
          type="button"
        >
          ← Back to project list
        </button>

        {isLocalWorkspace ? (
          <div className="project-detail-form-grid">
            <SelectField
              hint="Choose whether HGP should use a manual release label or detect it from project settings."
              label="Version source"
              onChange={(event) => {
                const versionSource = event.currentTarget
                  .value as OnDemandReleaseVersionSource;
                setDraft((current) => ({ ...current, versionSource }));
              }}
              options={VERSION_SOURCE_OPTIONS}
              value={draft.versionSource}
            />
            <div className="start-release-screen__version-row">
              <TextField
                autoFocus
                disabled={draft.versionSource !== "manual"}
                error={validationErrors.releaseVersion}
                hint={resolveVersionHint(draft.versionSource, detectedVersion)}
                label="Release version"
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
                  label="Reload detected version"
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
              Quick release is available for local workspace projects.
            </p>
            <p className="feed-state__copy">
              Open the project list to queue managed repository releases from
              the project detail screen.
            </p>
          </div>
        )}
      </div>
    </ScreenScaffold>
  );
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

function validateReleaseDraft(draft: ReleaseDraft): ReleaseValidationErrors {
  if (draft.versionSource !== "manual") {
    return {};
  }

  return draft.releaseVersion.trim()
    ? {}
    : {
        releaseVersion:
          "Release version is required for manual local dispatch.",
      };
}

// Builds the contextual hint string for the Release version field based on the
// current version source selection and the async detection state.
function resolveVersionHint(
  versionSource: OnDemandReleaseVersionSource,
  detected: DetectedVersion,
): string {
  if (versionSource === "manual") {
    return "Use the release label that should identify this local snapshot.";
  }
  switch (detected.status) {
    case "idle":
      return "Detecting…";
    case "loading":
      return "Detecting…";
    case "ready":
      return "Detected from project settings.";
    case "error":
      return `Detection failed: ${detected.message}`;
  }
}

function readErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return "The desktop shell could not queue the release.";
}
