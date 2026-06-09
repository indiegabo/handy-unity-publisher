import {
  startTransition,
  useCallback,
  useDeferredValue,
  useEffect,
  useState,
} from "react";

import { Button, IconButton } from "../Button";
import { SelectField, TextField, type SelectOption } from "../Field";
import ScreenScaffold from "../ScreenScaffold";
import { useLocalization, type Translate } from "../../LocalizationProvider";
import {
  dispatchOnDemandReleaseProcess,
  listOnDemandReleaseRemoteRefs,
  previewOnDemandReleaseVersion,
  type OnDemandReleaseRemoteRef,
  type ProcessPriority,
  type OnDemandReleaseVersionSource,
  type RepositoryInspectionEntry,
} from "../../services/projects";
import { buildProjectSourceDisplay } from "../../projectSourcePresentation";
import {
  buildProcessPriorityOptions,
  normalizeProcessPriority,
} from "./processPriority";

type ManagedReleaseSourceType = "branch" | "tag";

type ManagedReleaseDraft = {
  processPriority: ProcessPriority;
  sourceType: ManagedReleaseSourceType;
  sourceRef: string;
  releaseVersion: string;
  versionSource: OnDemandReleaseVersionSource;
};

type ManagedReleaseValidationErrors = {
  sourceRef?: string;
  releaseVersion?: string;
};

type DerivedVersion =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; value: string }
  | { status: "error"; message: string };

type RemoteRefState =
  | { status: "idle"; refs: OnDemandReleaseRemoteRef[] }
  | { status: "loading"; refs: OnDemandReleaseRemoteRef[] }
  | { status: "ready"; refs: OnDemandReleaseRemoteRef[] }
  | { status: "error"; refs: OnDemandReleaseRemoteRef[]; message: string };

export type ManagedRepositoryStartReleaseAdapterProps = {
  repository: RepositoryInspectionEntry;
  onBack: () => void;
  onCancel: () => void;
  onQueued: (gitTag: string, repositoryName: string) => void;
};

export function ManagedRepositoryStartReleaseAdapter({
  repository,
  onBack,
  onCancel,
  onQueued,
}: ManagedRepositoryStartReleaseAdapterProps) {
  const { t } = useLocalization();
  const [draft, setDraft] = useState<ManagedReleaseDraft>({
    processPriority: "low",
    sourceType: "branch",
    sourceRef: repository.default_branch ?? "",
    releaseVersion: "",
    versionSource: "manual",
  });
  const [validationErrors, setValidationErrors] =
    useState<ManagedReleaseValidationErrors>({});
  const [isQueueing, setIsQueueing] = useState(false);
  const [dispatchError, setDispatchError] = useState<string | null>(null);
  const [derivedVersion, setDerivedVersion] = useState<DerivedVersion>({
    status: "idle",
  });
  const [remoteRefs, setRemoteRefs] = useState<RemoteRefState>({
    status: "idle",
    refs: [],
  });
  const deferredSourceRef = useDeferredValue(draft.sourceRef);
  const sourceTypeOptions = buildSourceTypeOptions(t);
  const processPriorityOptions = buildProcessPriorityOptions(t);
  const versionSourceOptions = buildVersionSourceOptions(t, draft.sourceType);
  const sourceRefOptions = buildSourceRefOptions(
    t,
    draft.sourceType,
    remoteRefs,
    repository.default_branch,
  );

  useEffect(() => {
    let active = true;
    const sourceKind = mapManagedSourceTypeToKind(draft.sourceType);

    setRemoteRefs({ status: "loading", refs: [] });

    void listOnDemandReleaseRemoteRefs({
      repository_id: repository.repository_id,
      source_kind: sourceKind,
    })
      .then((refs) => {
        if (!active) {
          return;
        }

        startTransition(() => {
          setRemoteRefs({ status: "ready", refs });
          setDraft((current) => {
            if (current.sourceType !== draft.sourceType) {
              return current;
            }

            const sourceRef = chooseManagedSourceRef(
              current.sourceRef,
              current.sourceType,
              repository.default_branch,
              refs,
            );
            if (sourceRef === current.sourceRef) {
              return current;
            }

            return {
              ...current,
              sourceRef,
            };
          });
        });
      })
      .catch((error) => {
        if (!active) {
          return;
        }

        startTransition(() => {
          setRemoteRefs({
            status: "error",
            refs: [],
            message: readErrorMessage(t, error),
          });
          setDraft((current) => {
            if (current.sourceType !== draft.sourceType) {
              return current;
            }

            return {
              ...current,
              sourceRef: "",
            };
          });
        });
      });

    return () => {
      active = false;
    };
  }, [
    draft.sourceType,
    repository.default_branch,
    repository.repository_id,
    t,
  ]);

  const fetchDerivedVersion = useCallback(async () => {
    const sourceRef = deferredSourceRef.trim();
    if (!sourceRef) {
      setDerivedVersion({ status: "idle" });
      return;
    }

    setDerivedVersion({ status: "loading" });
    try {
      const version = await previewOnDemandReleaseVersion({
        repository_id: repository.repository_id,
        version_source: draft.versionSource,
        source_kind: mapManagedSourceTypeToKind(draft.sourceType),
        source_ref: sourceRef,
        local_path: null,
      });
      setDerivedVersion({ status: "ready", value: version });
    } catch (error) {
      setDerivedVersion({
        status: "error",
        message: readErrorMessage(t, error),
      });
    }
  }, [
    deferredSourceRef,
    draft.sourceType,
    draft.versionSource,
    repository.repository_id,
    t,
  ]);

  useEffect(() => {
    if (draft.versionSource === "manual") {
      setDerivedVersion({ status: "idle" });
      return;
    }

    if (!deferredSourceRef.trim()) {
      setDerivedVersion({ status: "idle" });
      return;
    }

    void fetchDerivedVersion();
  }, [deferredSourceRef, draft.versionSource, fetchDerivedVersion]);

  const handleQueueRelease = async () => {
    if (isQueueing) {
      return;
    }

    const errors = validateManagedReleaseDraft(t, draft);
    if (errors.sourceRef || errors.releaseVersion) {
      setValidationErrors(errors);
      return;
    }

    setIsQueueing(true);
    setDispatchError(null);

    try {
      const release = await dispatchOnDemandReleaseProcess({
        repository_id: repository.repository_id,
        release_version:
          draft.versionSource === "manual" ? draft.releaseVersion.trim() : null,
        version_source: draft.versionSource,
        source_kind: mapManagedSourceTypeToKind(draft.sourceType),
        source_ref: draft.sourceRef.trim(),
        local_path: null,
        process_priority: draft.processPriority,
        unity_executable_path_override: null,
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
            disabled={
              isQueueing ||
              remoteRefs.status !== "ready" ||
              remoteRefs.refs.length === 0
            }
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
                  "start_release.managed.actions.queue_release",
                  "Queue Managed Release",
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
              "start_release.managed.source_type.hint",
              "Choose whether HGP should resolve a branch or an exact tag from the managed repository.",
            )}
            label={t(
              "start_release.managed.source_type.label",
              "Repository source",
            )}
            onChange={(event) => {
              const sourceType = event.currentTarget
                .value as ManagedReleaseSourceType;
              setDraft((current) => ({
                ...current,
                sourceType,
                versionSource:
                  sourceType === "branch" &&
                  current.versionSource === "source_tag"
                    ? "manual"
                    : current.versionSource,
              }));
              setValidationErrors((current) => ({
                ...current,
                sourceRef: undefined,
              }));
            }}
            options={sourceTypeOptions}
            value={draft.sourceType}
          />
          <SelectField
            autoFocus
            disabled={
              isQueueing ||
              remoteRefs.status === "loading" ||
              remoteRefs.status === "error" ||
              remoteRefs.refs.length === 0
            }
            error={resolveSourceRefError(validationErrors, remoteRefs)}
            hint={buildSourceRefHint(
              t,
              draft.sourceType,
              repository.default_branch,
              remoteRefs,
            )}
            label={t(
              draft.sourceType === "branch"
                ? "start_release.managed.source_ref.branch_label"
                : "start_release.managed.source_ref.tag_label",
              draft.sourceType === "branch" ? "Branch" : "Tag",
            )}
            onChange={(event) => {
              const sourceRef = event.currentTarget.value;
              setDraft((current) => ({ ...current, sourceRef }));
              setValidationErrors((current) => ({
                ...current,
                sourceRef: undefined,
              }));
            }}
            options={sourceRefOptions}
            value={draft.sourceRef}
          />
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
            hint={buildVersionSourceHint(t, draft.sourceType)}
            label={t(
              "start_release.configure.version_source.label",
              "Version source",
            )}
            onChange={(event) => {
              const versionSource = event.currentTarget
                .value as OnDemandReleaseVersionSource;
              setDraft((current) => ({ ...current, versionSource }));
              setValidationErrors((current) => ({
                ...current,
                releaseVersion: undefined,
              }));
            }}
            options={versionSourceOptions}
            value={draft.versionSource}
          />
          <div className="start-release-screen__version-row">
            <TextField
              disabled={draft.versionSource !== "manual"}
              error={validationErrors.releaseVersion}
              hint={resolveDerivedVersionHint(
                t,
                draft.versionSource,
                derivedVersion,
              )}
              label={t(
                "start_release.configure.release_version.label",
                "Release version",
              )}
              onChange={(event) => {
                const releaseVersion = event.currentTarget.value;
                setDraft((current) => ({ ...current, releaseVersion }));
                setValidationErrors((current) => ({
                  ...current,
                  releaseVersion: undefined,
                }));
              }}
              placeholder={draft.versionSource !== "manual" ? "" : "v1.2.3"}
              value={
                draft.versionSource === "manual"
                  ? draft.releaseVersion
                  : derivedVersion.status === "ready"
                    ? derivedVersion.value
                    : ""
              }
            />
            {draft.versionSource !== "manual" ? (
              <IconButton
                className="start-release-screen__version-reload"
                disabled={
                  isQueueing ||
                  remoteRefs.status !== "ready" ||
                  derivedVersion.status === "loading" ||
                  !draft.sourceRef.trim()
                }
                icon="refresh"
                label={t(
                  "start_release.managed.actions.reload_version",
                  "Reload derived version",
                )}
                onClick={() => void fetchDerivedVersion()}
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

function buildSourceTypeOptions(t: Translate): SelectOption[] {
  return [
    {
      label: t("start_release.managed.source_type.branch", "Branch"),
      value: "branch",
    },
    {
      label: t("start_release.managed.source_type.tag", "Tag"),
      value: "tag",
    },
  ];
}

function buildVersionSourceOptions(
  t: Translate,
  sourceType: ManagedReleaseSourceType,
): SelectOption[] {
  const options: SelectOption[] = [
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

  if (sourceType === "tag") {
    options.push({
      label: t(
        "start_release.managed.version_source.source_tag",
        "Use selected tag value",
      ),
      value: "source_tag",
    });
  }

  return options;
}

function buildSourceRefOptions(
  t: Translate,
  sourceType: ManagedReleaseSourceType,
  remoteRefs: RemoteRefState,
  defaultBranch: string | null,
): SelectOption[] {
  if (remoteRefs.status === "loading" || remoteRefs.status === "idle") {
    return [
      {
        disabled: true,
        label: t(
          "start_release.managed.source_ref.loading",
          "Loading remote refs...",
        ),
        value: "",
      },
    ];
  }

  if (remoteRefs.status === "error") {
    return [
      {
        disabled: true,
        label: t(
          "start_release.managed.source_ref.unavailable",
          "Remote refs unavailable",
        ),
        value: "",
      },
    ];
  }

  if (remoteRefs.refs.length === 0) {
    return [
      {
        disabled: true,
        label:
          sourceType === "branch"
            ? t(
                "start_release.managed.source_ref.empty.branch",
                "No remote branches were detected.",
              )
            : t(
                "start_release.managed.source_ref.empty.tag",
                "No remote tags were detected.",
              ),
        value: "",
      },
    ];
  }

  return remoteRefs.refs.map((gitRef) => ({
    label:
      sourceType === "branch" && defaultBranch && gitRef.name === defaultBranch
        ? t(
            "start_release.managed.source_ref.branch_default_option",
            "{{branch}} (default)",
            { branch: gitRef.name },
          )
        : gitRef.name,
    title: gitRef.commit,
    value: gitRef.name,
  }));
}

function chooseManagedSourceRef(
  currentSourceRef: string,
  sourceType: ManagedReleaseSourceType,
  defaultBranch: string | null,
  remoteRefs: OnDemandReleaseRemoteRef[],
) {
  const normalizedCurrent = currentSourceRef.trim();
  if (
    normalizedCurrent &&
    remoteRefs.some((gitRef) => gitRef.name === normalizedCurrent)
  ) {
    return normalizedCurrent;
  }

  if (
    sourceType === "branch" &&
    defaultBranch &&
    remoteRefs.some((gitRef) => gitRef.name === defaultBranch)
  ) {
    return defaultBranch;
  }

  return remoteRefs[0]?.name ?? "";
}

function validateManagedReleaseDraft(
  t: Translate,
  draft: ManagedReleaseDraft,
): ManagedReleaseValidationErrors {
  const errors: ManagedReleaseValidationErrors = {};

  if (!draft.sourceRef.trim()) {
    errors.sourceRef = t(
      draft.sourceType === "branch"
        ? "start_release.managed.validation.branch_required"
        : "start_release.managed.validation.tag_required",
      draft.sourceType === "branch"
        ? "Branch name is required for managed release dispatch."
        : "Tag name is required for managed release dispatch.",
    );
  }

  if (draft.versionSource === "manual" && !draft.releaseVersion.trim()) {
    errors.releaseVersion = t(
      "start_release.managed.validation.release_version_required",
      "Release version is required for manual managed dispatch.",
    );
  }

  return errors;
}

function buildVersionSourceHint(
  t: Translate,
  sourceType: ManagedReleaseSourceType,
) {
  return sourceType === "tag"
    ? t(
        "start_release.managed.version_source.hint.tag",
        "Choose a manual version, detect it from the selected tag contents, or reuse the selected tag value.",
      )
    : t(
        "start_release.managed.version_source.hint.branch",
        "Choose a manual version or detect it from the selected branch contents.",
      );
}

function buildSourceRefHint(
  t: Translate,
  sourceType: ManagedReleaseSourceType,
  defaultBranch: string | null,
  remoteRefs: RemoteRefState,
) {
  if (remoteRefs.status === "loading" || remoteRefs.status === "idle") {
    return t(
      "start_release.managed.source_ref.loading",
      "Loading remote refs...",
    );
  }

  if (remoteRefs.status === "ready" && remoteRefs.refs.length === 0) {
    return sourceType === "branch"
      ? t(
          "start_release.managed.source_ref.empty.branch",
          "No remote branches were detected.",
        )
      : t(
          "start_release.managed.source_ref.empty.tag",
          "No remote tags were detected.",
        );
  }

  if (sourceType === "branch") {
    return defaultBranch
      ? t(
          "start_release.managed.source_ref.hint.branch_default",
          "Remote branch to release from. Default branch: {{branch}}.",
          { branch: defaultBranch },
        )
      : t(
          "start_release.managed.source_ref.hint.branch",
          "Remote branch to release from.",
        );
  }

  return t(
    "start_release.managed.source_ref.hint.tag",
    "Exact remote tag to release from.",
  );
}

function resolveSourceRefError(
  validationErrors: ManagedReleaseValidationErrors,
  remoteRefs: RemoteRefState,
) {
  if (remoteRefs.status === "error") {
    return remoteRefs.message;
  }

  return validationErrors.sourceRef;
}

function resolveDerivedVersionHint(
  t: Translate,
  versionSource: OnDemandReleaseVersionSource,
  derivedVersion: DerivedVersion,
) {
  if (versionSource === "manual") {
    return t(
      "start_release.managed.release_version.manual_hint",
      "Use the release label that should identify this managed dispatch.",
    );
  }

  if (versionSource === "source_tag") {
    switch (derivedVersion.status) {
      case "idle":
        return t(
          "start_release.managed.release_version.source_tag.idle",
          "Using the selected tag value after remote validation.",
        );
      case "loading":
        return t(
          "start_release.managed.release_version.detecting",
          "Resolving from the managed repository...",
        );
      case "ready":
        return t(
          "start_release.managed.release_version.source_tag.ready",
          "Using the selected tag value.",
        );
      case "error":
        return t(
          "start_release.managed.release_version.detect_failed",
          "Resolution failed: {{message}}",
          { message: derivedVersion.message },
        );
    }
  }

  switch (derivedVersion.status) {
    case "idle":
      return t(
        "start_release.managed.release_version.project_settings.idle",
        "Resolving bundleVersion from the selected repository source.",
      );
    case "loading":
      return t(
        "start_release.managed.release_version.detecting",
        "Resolving from the managed repository...",
      );
    case "ready":
      return t(
        "start_release.managed.release_version.project_settings.ready",
        "Detected from the selected repository source.",
      );
    case "error":
      return t(
        "start_release.managed.release_version.detect_failed",
        "Resolution failed: {{message}}",
        { message: derivedVersion.message },
      );
  }
}

function mapManagedSourceTypeToKind(
  sourceType: ManagedReleaseSourceType,
): "managed_ref" | "managed_tag" {
  return sourceType === "tag" ? "managed_tag" : "managed_ref";
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
