import { startTransition, useCallback, useEffect, useState } from "react";

import { Button, IconButton } from "../Button";
import { SelectField, TextField, type SelectOption } from "../Field";
import ScreenScaffold from "../ScreenScaffold";
import { useLocalization, type Translate } from "../../LocalizationProvider";
import {
  dispatchOnDemandReleaseProcess,
  readProjectSettingsVersion,
  type ProcessPriority,
  type OnDemandReleaseVersionSource,
  type RepositoryInspectionEntry,
} from "../../services/projects";
import { buildProjectSourceDisplay } from "../../projectSourcePresentation";
import {
  buildProcessPriorityOptions,
  normalizeProcessPriority,
} from "./processPriority";

type ReleaseDraft = {
  processPriority: ProcessPriority;
  releaseVersion: string;
  versionSource: OnDemandReleaseVersionSource;
};

type ReleaseValidationErrors = {
  releaseVersion?: string;
};

type DetectedVersion =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; value: string }
  | { status: "error"; message: string };

export type LocalWorkspaceStartReleaseAdapterProps = {
  repository: RepositoryInspectionEntry;
  onBack: () => void;
  onCancel: () => void;
  onQueued: (gitTag: string, repositoryName: string) => void;
};

export function LocalWorkspaceStartReleaseAdapter({
  repository,
  onBack,
  onCancel,
  onQueued,
}: LocalWorkspaceStartReleaseAdapterProps) {
  const { t } = useLocalization();
  const [draft, setDraft] = useState<ReleaseDraft>({
    processPriority: "low",
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
  const processPriorityOptions = buildProcessPriorityOptions(t);
  const versionSourceOptions = buildVersionSourceOptions(t);

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
  }, [repository.local_path, t]);

  useEffect(() => {
    if (draft.versionSource === "project_settings") {
      void fetchDetectedVersion();
    }
  }, [draft.versionSource, fetchDetectedVersion]);

  const handleQueueRelease = async () => {
    if (isQueueing) {
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
        process_priority: draft.processPriority,
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
              ? t("start_release.configure.actions.queueing", "Queueing...")
              : t(
                  "start_release.configure.actions.queue_local_release",
                  "Queue Local Release",
                )}
          </Button>
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
          {t("start_release.configure.actions.back", "← Back to project list")}
        </button>

        <div className="project-detail-form-grid">
          <SelectField
            hint={t(
              "start_release.configure.process_priority.hint",
              "Controls how aggressively the host schedules this release and its jobs. Lower priority reduces machine impact but can lengthen build and publish time.",
            )}
            label={t(
              "start_release.configure.process_priority.label",
              "Release process priority",
            )}
            onChange={(event) => {
              const processPriority = normalizeProcessPriority(
                event.currentTarget.value,
              );
              setDraft((current) => ({ ...current, processPriority }));
            }}
            options={processPriorityOptions}
            value={draft.processPriority}
          />
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
      </div>
    </ScreenScaffold>
  );
}

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
      return t(
        "start_release.configure.release_version.detecting",
        "Detecting...",
      );
    case "loading":
      return t(
        "start_release.configure.release_version.detecting",
        "Detecting...",
      );
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
