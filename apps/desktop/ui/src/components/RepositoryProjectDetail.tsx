import { startTransition, useEffect, useEffectEvent, useState } from "react";

import { Button } from "./Button";
import { SelectField, TextField } from "./Field";
import { PathPickerField } from "./PathPickerField";
import { Badge, SurfacePanel } from "./Surface";
import {
  loadRepositoryInspection,
  updateRepositoryProject,
  type RepositoryInspectionEntry,
  type UpdateRepositoryProjectInput,
} from "../services/projects";

type RepositoryProjectDetailProps = {
  repositoryId: number;
};

type RepositoryProjectDraft = {
  name: string;
  repositoryUrl: string;
  defaultBranch: string;
  artifactsRootOverride: string;
  workspaceRootOverride: string;
  pollingIntervalSeconds: string;
  enabled: "enabled" | "disabled";
};

type RepositoryProjectValidationErrors = {
  name?: string;
  repositoryUrl?: string;
  pollingIntervalSeconds?: string;
};

const MIN_PROJECT_POLL_INTERVAL_SECONDS = 5;
const PROJECT_STATUS_OPTIONS = [
  { label: "Enabled", value: "enabled" },
  { label: "Disabled", value: "disabled" },
] as const;

export function RepositoryProjectDetail({
  repositoryId,
}: RepositoryProjectDetailProps) {
  const [repository, setRepository] =
    useState<RepositoryInspectionEntry | null>(null);
  const [draft, setDraft] = useState<RepositoryProjectDraft | null>(null);
  const [validationErrors, setValidationErrors] =
    useState<RepositoryProjectValidationErrors>({});
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saveMessage, setSaveMessage] = useState<string | null>(null);

  const resolveRepositoryDetail = useEffectEvent(async () => {
    const inspection = await loadRepositoryInspection();

    return (
      inspection.repositories.find(
        (entry) => entry.repository_id === repositoryId,
      ) ?? null
    );
  });

  const loadRepositoryDetail = useEffectEvent(async (showLoading = true) => {
    if (showLoading) {
      setIsLoading(true);
    }

    try {
      const matchingRepository = await resolveRepositoryDetail();

      startTransition(() => {
        setRepository(matchingRepository);
        setDraft(
          matchingRepository
            ? buildRepositoryProjectDraft(matchingRepository)
            : null,
        );
        setValidationErrors({});
        setError(null);
        setIsLoading(false);
        if (showLoading) {
          setSaveError(null);
          setSaveMessage(null);
        }
      });
    } catch (loadError) {
      startTransition(() => {
        setError(buildProjectDetailErrorMessage(loadError));
        setIsLoading(false);
      });
    }
  });

  useEffect(() => {
    void loadRepositoryDetail(true);
  }, [repositoryId]);

  const handleDraftFieldChange = useEffectEvent(
    (fieldName: keyof RepositoryProjectDraft, value: string) => {
      startTransition(() => {
        setDraft((currentDraft) => {
          if (!currentDraft) {
            return currentDraft;
          }

          return {
            ...currentDraft,
            [fieldName]: value,
          } as RepositoryProjectDraft;
        });
      });
    },
  );

  const handleSaveProject = useEffectEvent(async () => {
    if (!repository || !draft || isSaving) {
      return;
    }

    const nextValidationErrors = validateRepositoryProjectDraft(draft);
    if (hasValidationErrors(nextValidationErrors)) {
      startTransition(() => {
        setValidationErrors(nextValidationErrors);
        setSaveError(null);
        setSaveMessage(null);
      });
      return;
    }

    setIsSaving(true);
    setSaveError(null);
    setSaveMessage(null);

    try {
      await updateRepositoryProject(
        buildRepositoryProjectUpdateInput(repository.repository_id, draft),
      );

      const refreshedRepository = await resolveRepositoryDetail();
      if (!refreshedRepository) {
        throw new Error("The updated project could not be reloaded.");
      }

      startTransition(() => {
        setRepository(refreshedRepository);
        setDraft(buildRepositoryProjectDraft(refreshedRepository));
        setValidationErrors({});
        setError(null);
        setIsLoading(false);
        setSaveMessage(`Saved changes for ${refreshedRepository.repository_name}.`);
      });
    } catch (saveProjectError) {
      startTransition(() => {
        setSaveError(buildProjectSaveErrorMessage(saveProjectError));
      });
    } finally {
      startTransition(() => {
        setIsSaving(false);
      });
    }
  });

  const handleReloadProject = useEffectEvent(() => {
    void loadRepositoryDetail(true);
  });

  if (isLoading) {
    return (
      <div className="project-detail-shell">
        <div className="feed-state">
          <p className="feed-state__title">Loading project detail...</p>
          <p className="feed-state__copy">
            The shell is resolving the repository configuration that was just
            created.
          </p>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="project-detail-shell">
        <p className="feed-banner feed-banner--error">{error}</p>
      </div>
    );
  }

  if (!repository) {
    return (
      <div className="project-detail-shell">
        <div className="feed-state">
          <p className="feed-state__title">Project not found.</p>
          <p className="feed-state__copy">
            The repository was created, but the current inspection payload does
            not include it yet.
          </p>
        </div>
      </div>
    );
  }

  const hasUnsavedChanges = repository && draft
    ? isRepositoryProjectDraftChanged(repository, draft)
    : false;

  return (
    <div className="project-detail-shell">
      {saveMessage ? <p className="notice-banner">{saveMessage}</p> : null}
      {saveError ? (
        <p className="feed-banner feed-banner--error">{saveError}</p>
      ) : null}

      <SurfacePanel
        actions={
          <div className="project-detail-toolbar">
            <Button
              leadingIcon="refresh"
              onClick={handleReloadProject}
              size="sm"
              variant="secondary"
            >
              Reload
            </Button>
            <Button
              disabled={!hasUnsavedChanges || isSaving}
              onClick={() => void handleSaveProject()}
              size="sm"
              variant="primary"
            >
              {isSaving ? "Saving..." : "Save Changes"}
            </Button>
          </div>
        }
        description="Edit the repository identity, cadence, and runtime-managed paths."
        eyebrow="Repository Project"
        title="Edit Project"
      >
        <div className="project-detail-summary">
          <Badge tone={draft?.enabled === "enabled" ? "strong" : "muted"}>
            {draft?.enabled === "enabled" ? "enabled" : "disabled"}
          </Badge>
          <Badge tone="neutral">
            Poll every {repository.polling_interval_seconds}s
          </Badge>
          <Badge tone="muted">
            {repository.enabled_build_target_count} active target
            {repository.enabled_build_target_count === 1 ? "" : "s"}
          </Badge>
          <Badge tone="muted">
            {repository.credentials
              ? `credential: ${repository.credentials.name}`
              : "no repository credential"}
          </Badge>
        </div>

        {draft ? (
          <div className="project-detail-form">
            <div className="project-detail-form-grid">
              <TextField
                error={validationErrors.name}
                label="Project name"
                onChange={(event) =>
                  handleDraftFieldChange("name", event.target.value)
                }
                placeholder="Project name"
                value={draft.name}
              />

              <TextField
                error={validationErrors.repositoryUrl}
                label="Repository URL"
                onChange={(event) =>
                  handleDraftFieldChange("repositoryUrl", event.target.value)
                }
                placeholder="https://example.com/repository.git"
                value={draft.repositoryUrl}
              />

              <TextField
                hint="Optional"
                label="Default branch"
                onChange={(event) =>
                  handleDraftFieldChange("defaultBranch", event.target.value)
                }
                placeholder="main"
                value={draft.defaultBranch}
              />

              <TextField
                error={validationErrors.pollingIntervalSeconds}
                hint="Minimum 5s"
                inputMode="numeric"
                label="Polling interval"
                min={MIN_PROJECT_POLL_INTERVAL_SECONDS}
                onChange={(event) =>
                  handleDraftFieldChange(
                    "pollingIntervalSeconds",
                    event.target.value,
                  )
                }
                placeholder="300"
                type="number"
                value={draft.pollingIntervalSeconds}
              />

              <SelectField
                label="Project status"
                onChange={(event) =>
                  handleDraftFieldChange(
                    "enabled",
                    event.target.value as RepositoryProjectDraft["enabled"],
                  )
                }
                options={PROJECT_STATUS_OPTIONS}
                value={draft.enabled}
              />

              <div className="project-detail-form-grid__span-full">
                <PathPickerField
                  buttonLabel="Pick artifacts root"
                  clearLabel="Reset"
                  clearable
                  dialogTitle="Select artifacts root override"
                  disabled={isSaving}
                  hint="Optional repository-specific override"
                  label="Artifacts root override"
                  onClear={() =>
                    handleDraftFieldChange("artifactsRootOverride", "")
                  }
                  onError={(pickError) => {
                    setSaveError(buildProjectSaveErrorMessage(pickError));
                  }}
                  onPathPicked={(path) =>
                    handleDraftFieldChange("artifactsRootOverride", path)
                  }
                  pickerKind="directory"
                  placeholder="Uses the runtime artifacts root when empty"
                  value={draft.artifactsRootOverride}
                />
              </div>

              <div className="project-detail-form-grid__span-full">
                <PathPickerField
                  buttonLabel="Pick workspace root"
                  clearLabel="Reset"
                  clearable
                  dialogTitle="Select workspace root override"
                  disabled={isSaving}
                  hint="Optional repository-specific checkout root"
                  label="Workspace root override"
                  onClear={() =>
                    handleDraftFieldChange("workspaceRootOverride", "")
                  }
                  onError={(pickError) => {
                    setSaveError(buildProjectSaveErrorMessage(pickError));
                  }}
                  onPathPicked={(path) =>
                    handleDraftFieldChange("workspaceRootOverride", path)
                  }
                  pickerKind="directory"
                  placeholder="Uses the runtime workspace root when empty"
                  value={draft.workspaceRootOverride}
                />
              </div>
            </div>
          </div>
        ) : null}
      </SurfacePanel>

      <SurfacePanel
        description="Host-native targets registered for this repository."
        eyebrow="Execution"
        title="Build Targets"
      >
        {repository.build_targets.length === 0 ? (
          <div className="feed-state">
            <p className="feed-state__title">No build targets configured.</p>
            <p className="feed-state__copy">
              This repository will not produce build work until at least one
              target is enabled.
            </p>
          </div>
        ) : (
          <div className="project-detail-target-list">
            {repository.build_targets.map((target) => (
              <section
                className="project-detail-target-card"
                key={target.build_target_id}
              >
                <div className="project-detail-target-card__header">
                  <div className="project-detail-target-card__title-block">
                    <h3 className="project-detail-target-card__title">
                      {target.target_name}
                    </h3>
                    <p className="project-detail-target-card__copy">
                      {target.build_method || "Build method not configured"}
                    </p>
                  </div>
                  <div className="project-detail-target-card__badges">
                    <Badge tone="neutral">{target.platform}</Badge>
                    <Badge
                      tone={
                        target.diagnostic_status === "ready"
                          ? "strong"
                          : "muted"
                      }
                    >
                      {target.diagnostic_status.replace(/_/g, " ")}
                    </Badge>
                  </div>
                </div>
                <p className="project-detail-target-card__copy project-detail-target-card__copy--muted">
                  {target.diagnostic_message}
                </p>
              </section>
            ))}
          </div>
        )}
      </SurfacePanel>

      <SurfacePanel
        description="Queue and execution backlog for the registered repository."
        eyebrow="Automation"
        title="Runtime Status"
      >
        <div className="project-detail-status-grid">
          <div className="project-detail-status-card">
            <strong>{repository.pending_release_count}</strong>
            <span>Pending releases</span>
          </div>
          <div className="project-detail-status-card">
            <strong>{repository.queued_build_runs}</strong>
            <span>Queued builds</span>
          </div>
          <div className="project-detail-status-card">
            <strong>{repository.running_build_runs}</strong>
            <span>Running builds</span>
          </div>
          <div className="project-detail-status-card">
            <strong>{repository.running_publish_runs}</strong>
            <span>Running publishes</span>
          </div>
        </div>
      </SurfacePanel>
    </div>
  );
}

function buildProjectDetailErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return "The desktop shell could not load the project detail.";
}

function buildProjectSaveErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return "The desktop shell could not save the project changes.";
}

function buildRepositoryProjectDraft(
  repository: RepositoryInspectionEntry,
): RepositoryProjectDraft {
  return {
    name: repository.repository_name,
    repositoryUrl: repository.repo_url,
    defaultBranch: repository.default_branch ?? "",
    artifactsRootOverride: repository.artifacts_root_override ?? "",
    workspaceRootOverride: repository.workspace_root_override ?? "",
    pollingIntervalSeconds: String(repository.polling_interval_seconds),
    enabled: repository.enabled ? "enabled" : "disabled",
  };
}

function validateRepositoryProjectDraft(
  draft: RepositoryProjectDraft,
): RepositoryProjectValidationErrors {
  const errors: RepositoryProjectValidationErrors = {};

  if (!draft.name.trim()) {
    errors.name = "Project name is required.";
  }

  const repositoryUrl = draft.repositoryUrl.trim();
  if (!repositoryUrl) {
    errors.repositoryUrl = "Repository URL is required.";
  } else if (
    !repositoryUrl.startsWith("https://") &&
    !repositoryUrl.startsWith("http://")
  ) {
    errors.repositoryUrl = "Repository URL must use http:// or https://.";
  }

  const pollingIntervalSeconds = Number(draft.pollingIntervalSeconds);
  if (!Number.isInteger(pollingIntervalSeconds)) {
    errors.pollingIntervalSeconds = "Polling interval must be an integer.";
  } else if (pollingIntervalSeconds < MIN_PROJECT_POLL_INTERVAL_SECONDS) {
    errors.pollingIntervalSeconds =
      "Polling interval must be at least 5 seconds.";
  }

  return errors;
}

function hasValidationErrors(errors: RepositoryProjectValidationErrors) {
  return Boolean(
    errors.name || errors.repositoryUrl || errors.pollingIntervalSeconds,
  );
}

function buildRepositoryProjectUpdateInput(
  repositoryId: number,
  draft: RepositoryProjectDraft,
): UpdateRepositoryProjectInput {
  return {
    repository_id: repositoryId,
    name: draft.name.trim(),
    repository_url: draft.repositoryUrl.trim(),
    default_branch: normalizeOptionalDraftValue(draft.defaultBranch),
    artifacts_root_override: normalizeOptionalDraftValue(
      draft.artifactsRootOverride,
    ),
    workspace_root_override: normalizeOptionalDraftValue(
      draft.workspaceRootOverride,
    ),
    polling_interval_seconds: Number(draft.pollingIntervalSeconds),
    enabled: draft.enabled === "enabled",
  };
}

function isRepositoryProjectDraftChanged(
  repository: RepositoryInspectionEntry,
  draft: RepositoryProjectDraft,
) {
  const persistedDraft = buildRepositoryProjectDraft(repository);

  return (
    persistedDraft.name.trim() !== draft.name.trim() ||
    persistedDraft.repositoryUrl.trim() !== draft.repositoryUrl.trim() ||
    persistedDraft.defaultBranch.trim() !== draft.defaultBranch.trim() ||
    persistedDraft.artifactsRootOverride.trim() !==
      draft.artifactsRootOverride.trim() ||
    persistedDraft.workspaceRootOverride.trim() !==
      draft.workspaceRootOverride.trim() ||
    persistedDraft.pollingIntervalSeconds !== draft.pollingIntervalSeconds ||
    persistedDraft.enabled !== draft.enabled
  );
}

function normalizeOptionalDraftValue(value: string) {
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}
