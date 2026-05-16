import {
  startTransition,
  type ReactNode,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";

import { Button } from "./Button";
import { SelectField, TextField } from "./Field";
import { PathPickerField } from "./PathPickerField";
import { RepositoryEngineField } from "./RepositoryEngineField";
import { Badge } from "./Surface";
import { VerticalAccordion } from "./VerticalAccordion";
import {
  loadRepositoryInspection,
  updateRepositoryProject,
  validateUnityExecutablePath,
  type RepositoryEngineKind,
  type RepositoryInspectionEntry,
  type UnityExecutableValidation,
  type UpdateRepositoryProjectInput,
} from "../services/projects";

type RepositoryProjectDetailProps = {
  repositoryId: number;
};

type RepositoryProjectBuildTargetDraft = {
  id: string;
  buildTargetId: number | null;
  name: string;
  targetPlatform: string;
  buildMethod: string;
  unityExecutablePath: string;
};

type RepositoryProjectDraft = {
  engineKind: RepositoryEngineKind;
  name: string;
  repositoryUrl: string;
  defaultBranch: string;
  artifactsRootOverride: string;
  workspaceRootOverride: string;
  pollingIntervalSeconds: string;
  enabled: "enabled" | "disabled";
  buildTargets: RepositoryProjectBuildTargetDraft[];
};

type RepositoryProjectBuildTargetValidationErrors = {
  name?: string;
  targetPlatform?: string;
  buildMethod?: string;
  unityExecutablePath?: string;
};

type RepositoryProjectValidationErrors = {
  engineKind?: string;
  name?: string;
  repositoryUrl?: string;
  pollingIntervalSeconds?: string;
  buildTargetsRoot?: string;
  buildTargets: Record<string, RepositoryProjectBuildTargetValidationErrors>;
};

type RepositoryProjectFieldName = Exclude<
  keyof RepositoryProjectDraft,
  "buildTargets"
>;

type ProjectDetailSectionKey = "project" | "targets" | "automation";

type ValidationTimerMap = Record<string, number | undefined>;

type RepositoryProjectTargetEditorState = {
  buildTargets: RepositoryProjectBuildTargetDraft[];
  pathDiagnostics: Record<string, UnityExecutableValidation | null>;
  expandedTargetIds: Record<string, boolean>;
  nextBuildTargetIndex: number;
};

const MIN_PROJECT_POLL_INTERVAL_SECONDS = 5;
const PROJECT_STATUS_OPTIONS = [
  { label: "Enabled", value: "enabled" },
  { label: "Disabled", value: "disabled" },
] as const;
const PLATFORM_OPTIONS = [
  { label: "Select a Unity target", value: "" },
  { label: "Windows", value: "StandaloneWindows64" },
  { label: "Linux", value: "StandaloneLinux64" },
  { label: "macOS", value: "StandaloneOSX" },
  { label: "WebGL", value: "WebGL" },
  { label: "Android", value: "Android" },
] as const;
const DEFAULT_SECTION_OPEN_STATE: Record<ProjectDetailSectionKey, boolean> = {
  project: true,
  targets: true,
  automation: true,
};

export function RepositoryProjectDetail({
  repositoryId,
}: RepositoryProjectDetailProps) {
  const [repository, setRepository] =
    useState<RepositoryInspectionEntry | null>(null);
  const [draft, setDraft] = useState<RepositoryProjectDraft | null>(null);
  const [validationErrors, setValidationErrors] =
    useState<RepositoryProjectValidationErrors>(createEmptyValidationErrors);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saveMessage, setSaveMessage] = useState<string | null>(null);
  const [pathDiagnostics, setPathDiagnostics] = useState<
    Record<string, UnityExecutableValidation | null>
  >({});
  const [validatingTargets, setValidatingTargets] = useState<
    Record<string, boolean>
  >({});
  const [expandedTargetIds, setExpandedTargetIds] = useState<
    Record<string, boolean>
  >({});
  const [sectionOpenState, setSectionOpenState] = useState(
    DEFAULT_SECTION_OPEN_STATE,
  );
  const nextBuildTargetIdRef = useRef(1);
  const validationTimersRef = useRef<ValidationTimerMap>({});
  const validationTokenRef = useRef<Record<string, number>>({});

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
      const targetEditorState = matchingRepository
        ? buildRepositoryProjectTargetEditorState(matchingRepository)
        : createEmptyTargetEditorState();

      startTransition(() => {
        setRepository(matchingRepository);
        setDraft(
          matchingRepository
            ? buildRepositoryProjectDraft(
                matchingRepository,
                targetEditorState.buildTargets,
              )
            : null,
        );
        setValidationErrors(createEmptyValidationErrors());
        setPathDiagnostics(targetEditorState.pathDiagnostics);
        setValidatingTargets({});
        setExpandedTargetIds(targetEditorState.expandedTargetIds);
        setError(null);
        setIsLoading(false);
        if (showLoading) {
          setSaveError(null);
          setSaveMessage(null);
        }
      });

      nextBuildTargetIdRef.current = targetEditorState.nextBuildTargetIndex;
    } catch (loadError) {
      startTransition(() => {
        setError(buildProjectDetailErrorMessage(loadError));
        setIsLoading(false);
      });
    }
  });

  useEffect(() => {
    void loadRepositoryDetail(true);

    return () => {
      for (const timerId of Object.values(validationTimersRef.current)) {
        if (timerId !== undefined) {
          window.clearTimeout(timerId);
        }
      }
    };
  }, [repositoryId]);

  const handleDraftFieldChange = useEffectEvent(
    (fieldName: RepositoryProjectFieldName, value: string) => {
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

  const updateBuildTarget = useEffectEvent(
    (targetId: string, patch: Partial<RepositoryProjectBuildTargetDraft>) => {
      startTransition(() => {
        setDraft((currentDraft) => {
          if (!currentDraft) {
            return currentDraft;
          }

          return {
            ...currentDraft,
            buildTargets: currentDraft.buildTargets.map((target) =>
              target.id === targetId ? { ...target, ...patch } : target,
            ),
          };
        });
      });
    },
  );

  const scheduleUnityExecutableValidation = useEffectEvent(
    (targetId: string, path: string, delayMillis = 250) => {
      const existingTimerId = validationTimersRef.current[targetId];
      if (existingTimerId !== undefined) {
        window.clearTimeout(existingTimerId);
      }

      validationTokenRef.current[targetId] =
        (validationTokenRef.current[targetId] ?? 0) + 1;
      const validationToken = validationTokenRef.current[targetId];
      const trimmedPath = path.trim();

      if (!trimmedPath) {
        startTransition(() => {
          setPathDiagnostics((current) => ({
            ...current,
            [targetId]: null,
          }));
          setValidatingTargets((current) => ({
            ...current,
            [targetId]: false,
          }));
        });
        return;
      }

      startTransition(() => {
        setValidatingTargets((current) => ({
          ...current,
          [targetId]: true,
        }));
      });

      validationTimersRef.current[targetId] = window.setTimeout(async () => {
        try {
          const diagnostics = await validateUnityExecutablePath(trimmedPath);
          if (validationTokenRef.current[targetId] !== validationToken) {
            return;
          }

          startTransition(() => {
            setPathDiagnostics((current) => ({
              ...current,
              [targetId]: diagnostics,
            }));
            setValidatingTargets((current) => ({
              ...current,
              [targetId]: false,
            }));
          });
        } catch (validationError) {
          if (validationTokenRef.current[targetId] !== validationToken) {
            return;
          }

          startTransition(() => {
            setPathDiagnostics((current) => ({
              ...current,
              [targetId]: {
                runner_family: "host-native",
                unity_executable_path: trimmedPath,
                unity_executable_exists: false,
                unity_executable_is_file: false,
                additional_argument_count: 0,
                environment_variable_count: 0,
                status: "validation_failed",
                message: buildProjectSaveErrorMessage(validationError),
              },
            }));
            setValidatingTargets((current) => ({
              ...current,
              [targetId]: false,
            }));
          });
        }
      }, delayMillis);
    },
  );

  const handlePickUnityExecutablePath = useEffectEvent(
    (targetId: string, selectedPath: string) => {
      updateBuildTarget(targetId, { unityExecutablePath: selectedPath });
      scheduleUnityExecutableValidation(targetId, selectedPath, 0);
    },
  );

  const handleAddBuildTarget = useEffectEvent(() => {
    const nextTarget = createEmptyBuildTargetDraft(nextBuildTargetIdRef.current);
    nextBuildTargetIdRef.current += 1;

    startTransition(() => {
      setDraft((currentDraft) => {
        if (!currentDraft) {
          return currentDraft;
        }

        return {
          ...currentDraft,
          buildTargets: [...currentDraft.buildTargets, nextTarget],
        };
      });
      setExpandedTargetIds((current) => ({
        ...current,
        [nextTarget.id]: true,
      }));
      setSectionOpenState((current) => ({
        ...current,
        targets: true,
      }));
    });
  });

  const handleRemoveBuildTarget = useEffectEvent((targetId: string) => {
    const existingTimerId = validationTimersRef.current[targetId];
    if (existingTimerId !== undefined) {
      window.clearTimeout(existingTimerId);
    }

    startTransition(() => {
      setDraft((currentDraft) => {
        if (!currentDraft) {
          return currentDraft;
        }

        return {
          ...currentDraft,
          buildTargets: currentDraft.buildTargets.filter(
            (target) => target.id !== targetId,
          ),
        };
      });
      setPathDiagnostics((current) => {
        const next = { ...current };
        delete next[targetId];
        return next;
      });
      setValidatingTargets((current) => {
        const next = { ...current };
        delete next[targetId];
        return next;
      });
      setExpandedTargetIds((current) => {
        const next = { ...current };
        delete next[targetId];
        return next;
      });
    });
  });

  const handleTargetAccordionChange = useEffectEvent(
    (targetId: string, nextOpen: boolean) => {
      startTransition(() => {
        setExpandedTargetIds((current) => ({
          ...current,
          [targetId]: nextOpen,
        }));
      });
    },
  );

  const handleSectionOpenChange = useEffectEvent(
    (sectionKey: ProjectDetailSectionKey, nextOpen: boolean) => {
      startTransition(() => {
        setSectionOpenState((current) => ({
          ...current,
          [sectionKey]: nextOpen,
        }));
      });
    },
  );

  const handleSaveProject = useEffectEvent(async () => {
    if (!repository || !draft || isSaving) {
      return;
    }

    const nextValidationErrors = validateRepositoryProjectDraft(
      draft,
      pathDiagnostics,
      validatingTargets,
    );
    if (hasValidationErrors(nextValidationErrors)) {
      const invalidTargetIds = collectInvalidTargetIds(nextValidationErrors);

      startTransition(() => {
        setValidationErrors(nextValidationErrors);
        setSaveError(null);
        setSaveMessage(null);
        setExpandedTargetIds((current) =>
          mergeExpandedTargetIds(current, invalidTargetIds),
        );
        setSectionOpenState((current) => ({
          ...current,
          project: true,
          targets:
            current.targets ||
            Boolean(nextValidationErrors.buildTargetsRoot || invalidTargetIds.length),
        }));
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

      const targetEditorState = buildRepositoryProjectTargetEditorState(
        refreshedRepository,
      );
      nextBuildTargetIdRef.current = targetEditorState.nextBuildTargetIndex;

      startTransition(() => {
        setRepository(refreshedRepository);
        setDraft(
          buildRepositoryProjectDraft(
            refreshedRepository,
            targetEditorState.buildTargets,
          ),
        );
        setValidationErrors(createEmptyValidationErrors());
        setPathDiagnostics(targetEditorState.pathDiagnostics);
        setValidatingTargets({});
        setExpandedTargetIds(targetEditorState.expandedTargetIds);
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
  const activeTargetCount = draft?.buildTargets.length ?? 0;
  const pollingIntervalLabel = draft?.pollingIntervalSeconds.trim() || String(
    repository.polling_interval_seconds,
  );

  return (
    <div className="project-detail-shell">
      {saveMessage ? <p className="notice-banner">{saveMessage}</p> : null}
      {saveError ? (
        <p className="feed-banner feed-banner--error">{saveError}</p>
      ) : null}

      <ProjectDetailSectionAccordion
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
        onOpenChange={(nextOpen) => handleSectionOpenChange("project", nextOpen)}
        open={sectionOpenState.project}
        title="Edit Project"
      >
        <div className="project-detail-summary">
          <Badge tone={draft?.enabled === "enabled" ? "strong" : "muted"}>
            {draft?.enabled === "enabled" ? "enabled" : "disabled"}
          </Badge>
          <Badge tone="neutral">engine: {draft?.engineKind ?? "unity"}</Badge>
          <Badge tone="neutral">
            Poll every {pollingIntervalLabel}s
          </Badge>
          <Badge tone="muted">
            {activeTargetCount} active target{activeTargetCount === 1 ? "" : "s"}
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

              <RepositoryEngineField
                error={validationErrors.engineKind}
                onChange={(event) =>
                  handleDraftFieldChange(
                    "engineKind",
                    event.target.value as RepositoryProjectDraft["engineKind"],
                  )
                }
                value={draft.engineKind}
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
      </ProjectDetailSectionAccordion>

      <ProjectDetailSectionAccordion
        actions={
          <Button
            disabled={isSaving}
            leadingIcon="plus"
            onClick={handleAddBuildTarget}
            size="sm"
            variant="secondary"
          >
            Add target
          </Button>
        }
        description="Host-native targets registered for this repository."
        eyebrow="Execution"
        onOpenChange={(nextOpen) => handleSectionOpenChange("targets", nextOpen)}
        open={sectionOpenState.targets}
        title="Build Targets"
      >
        {validationErrors.buildTargetsRoot ? (
          <p className="feed-banner feed-banner--error">
            {validationErrors.buildTargetsRoot}
          </p>
        ) : null}

        {draft && draft.buildTargets.length === 0 ? (
          <div className="feed-state">
            <p className="feed-state__title">No build targets configured.</p>
            <p className="feed-state__copy">
              This repository will not produce build work until at least one
              target is enabled.
            </p>
          </div>
        ) : (
          <div className="project-detail-target-list">
            {draft?.buildTargets.map((target, index) => {
              const diagnostics = pathDiagnostics[target.id];
              const fieldErrors = validationErrors.buildTargets[target.id] ?? {};

              return (
                <VerticalAccordion
                  bodyClassName="wizard-target-card__body"
                  className="wizard-target-card"
                  collapsedToggleLabel={`Expand build target ${index + 1}`}
                  expandedToggleLabel={`Collapse build target ${index + 1}`}
                  header={
                    <div className="wizard-target-card__header">
                      <div className="wizard-target-card__title-block">
                        <p className="wizard-target-card__eyebrow">
                          Build target {index + 1}
                        </p>
                        <h3 className="wizard-target-card__title">
                          {target.name.trim() || "Unnamed target"}
                        </h3>
                      </div>

                      <div className="wizard-target-card__actions">
                        <Badge tone="neutral">
                          {target.targetPlatform.trim() || "no Unity target"}
                        </Badge>
                        {diagnostics ? (
                          <Badge
                            tone={
                              diagnostics.status === "ready"
                                ? "strong"
                                : "muted"
                            }
                          >
                            {formatDiagnosticStatus(diagnostics.status)}
                          </Badge>
                        ) : null}
                        <Button
                          disabled={draft.buildTargets.length === 1 || isSaving}
                          onClick={() => handleRemoveBuildTarget(target.id)}
                          size="sm"
                          variant="ghost"
                        >
                          Remove
                        </Button>
                      </div>
                    </div>
                  }
                  key={target.id}
                  onOpenChange={(nextOpen) =>
                    handleTargetAccordionChange(target.id, nextOpen)
                  }
                  open={Boolean(expandedTargetIds[target.id])}
                  triggerMode="button"
                >
                  <div className="wizard-form-grid wizard-form-grid--targets">
                    <TextField
                      error={fieldErrors.name}
                      hint="Keep the target name stable. It becomes part of the artifact file name."
                      label="Target name"
                      onChange={(event) => {
                        updateBuildTarget(target.id, {
                          name: event.currentTarget.value,
                        });
                      }}
                      placeholder="Windows"
                      value={target.name}
                    />
                    <SelectField
                      error={fieldErrors.targetPlatform}
                      hint="This writes the Unity targetPlatform contract field directly."
                      label="Unity target platform"
                      onChange={(event) => {
                        updateBuildTarget(target.id, {
                          targetPlatform: normalizeUnityTargetPlatformValue(
                            event.currentTarget.value,
                          ),
                        });
                      }}
                      options={PLATFORM_OPTIONS}
                      value={normalizeUnityTargetPlatformValue(
                        target.targetPlatform,
                      )}
                    />
                    <TextField
                      error={fieldErrors.buildMethod}
                      hint="Point this at a real static Unity method, for example Builder.PerformWindows."
                      label="Unity build method"
                      onChange={(event) => {
                        updateBuildTarget(target.id, {
                          buildMethod: event.currentTarget.value,
                        });
                      }}
                      placeholder="Builder.PerformWindows"
                      value={target.buildMethod}
                    />
                    <PathPickerField
                      buttonLabel="Choose Unity executable"
                      disabled={isSaving}
                      dialogTitle="Select Unity Editor executable"
                      error={fieldErrors.unityExecutablePath}
                      filters={[
                        {
                          name: "Unity Editor",
                          extensions: ["exe", "app"],
                        },
                      ]}
                      hint="Select the host-local Unity Editor executable that should run this target."
                      label="Unity executable"
                      onError={(pickError) => {
                        setSaveError(buildProjectSaveErrorMessage(pickError));
                      }}
                      onPathPicked={(selectedPath) =>
                        handlePickUnityExecutablePath(target.id, selectedPath)
                      }
                      pickerKind="file"
                      placeholder="C:/Program Files/Unity/Hub/Editor/.../Unity.exe"
                      value={target.unityExecutablePath}
                    />

                    {diagnostics ? (
                      <p
                        className={joinClassNames(
                          "wizard-target-card__diagnostic",
                          diagnostics.status !== "ready" &&
                            "wizard-target-card__diagnostic--error",
                        )}
                      >
                        {diagnostics.message}
                      </p>
                    ) : null}

                    {validatingTargets[target.id] ? (
                      <p className="wizard-target-card__diagnostic">
                        Validating Unity executable path...
                      </p>
                    ) : null}
                  </div>
                </VerticalAccordion>
              );
            })}
          </div>
        )}
      </ProjectDetailSectionAccordion>

      <ProjectDetailSectionAccordion
        description="Queue and execution backlog for the registered repository."
        eyebrow="Automation"
        onOpenChange={(nextOpen) =>
          handleSectionOpenChange("automation", nextOpen)
        }
        open={sectionOpenState.automation}
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
      </ProjectDetailSectionAccordion>
    </div>
  );
}

function ProjectDetailSectionAccordion({
  actions,
  children,
  description,
  eyebrow,
  onOpenChange,
  open,
  title,
}: {
  actions?: ReactNode;
  children: ReactNode;
  description: string;
  eyebrow: string;
  onOpenChange: (nextOpen: boolean) => void;
  open: boolean;
  title: string;
}) {
  return (
    <VerticalAccordion
      bodyClassName="ui-panel__body project-detail-section-accordion__body"
      className="ui-panel project-detail-section-accordion"
      collapsedToggleLabel={`Expand ${title}`}
      expandedToggleLabel={`Collapse ${title}`}
      header={
        <div className="project-detail-section-accordion__header-content">
          <div className="ui-panel__title-block">
            <p className="ui-panel__eyebrow">{eyebrow}</p>
            <h2 className="ui-panel__title">{title}</h2>
            <p className="ui-panel__description">{description}</p>
          </div>
          {actions ? (
            <div className="project-detail-section-accordion__actions">
              {actions}
            </div>
          ) : null}
        </div>
      }
      headerClassName="project-detail-section-accordion__header"
      onOpenChange={onOpenChange}
      open={open}
      triggerMode="button"
    >
      {children}
    </VerticalAccordion>
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

function createEmptyValidationErrors(): RepositoryProjectValidationErrors {
  return {
    buildTargets: {},
  };
}

function createEmptyTargetEditorState(): RepositoryProjectTargetEditorState {
  return {
    buildTargets: [],
    pathDiagnostics: {},
    expandedTargetIds: {},
    nextBuildTargetIndex: 1,
  };
}

function createEmptyBuildTargetDraft(
  index: number,
): RepositoryProjectBuildTargetDraft {
  return {
    id: `target-${index}`,
    buildTargetId: null,
    name: "",
    targetPlatform: "",
    buildMethod: "",
    unityExecutablePath: "",
  };
}

function buildRepositoryProjectDraft(
  repository: RepositoryInspectionEntry,
  buildTargets: RepositoryProjectBuildTargetDraft[],
): RepositoryProjectDraft {
  return {
    engineKind: normalizeRepositoryEngineKind(repository.engine_kind),
    name: repository.repository_name,
    repositoryUrl: repository.repo_url,
    defaultBranch: repository.default_branch ?? "",
    artifactsRootOverride: repository.artifacts_root_override ?? "",
    workspaceRootOverride: repository.workspace_root_override ?? "",
    pollingIntervalSeconds: String(repository.polling_interval_seconds),
    enabled: repository.enabled ? "enabled" : "disabled",
    buildTargets,
  };
}

function buildRepositoryProjectTargetEditorState(
  repository: RepositoryInspectionEntry,
): RepositoryProjectTargetEditorState {
  const buildTargets: RepositoryProjectBuildTargetDraft[] = [];
  const pathDiagnostics: Record<string, UnityExecutableValidation | null> = {};
  const expandedTargetIds: Record<string, boolean> = {};

  for (const [index, target] of repository.build_targets
    .filter((buildTarget) => buildTarget.enabled)
    .entries()) {
    const targetId = `target-${index + 1}`;
    buildTargets.push({
      id: targetId,
      buildTargetId: target.build_target_id,
      name: target.target_name,
      targetPlatform: normalizeUnityTargetPlatformValue(
        target.unity_target_platform,
      ),
      buildMethod: target.unity_build_method ?? "",
      unityExecutablePath:
        target.host_native_diagnostics?.unity_executable_path ?? "",
    });
    pathDiagnostics[targetId] = target.host_native_diagnostics;
    expandedTargetIds[targetId] = true;
  }

  return {
    buildTargets,
    pathDiagnostics,
    expandedTargetIds,
    nextBuildTargetIndex: buildTargets.length + 1,
  };
}

function validateRepositoryProjectDraft(
  draft: RepositoryProjectDraft,
  pathDiagnostics: Record<string, UnityExecutableValidation | null>,
  validatingTargets: Record<string, boolean>,
): RepositoryProjectValidationErrors {
  const errors = createEmptyValidationErrors();

  if (draft.engineKind !== "unity") {
    errors.engineKind =
      "Only Unity is currently supported even though future engines are listed.";
  }

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

  if (draft.buildTargets.length === 0) {
    errors.buildTargetsRoot = "At least one build target is required.";
    return errors;
  }

  const seenNames = new Set<string>();
  for (const target of draft.buildTargets) {
    const fieldErrors: RepositoryProjectBuildTargetValidationErrors = {};
    const normalizedName = target.name.trim();
    if (!normalizedName) {
      fieldErrors.name = "Target name is required.";
    } else {
      const duplicateKey = normalizedName.toLocaleLowerCase();
      if (seenNames.has(duplicateKey)) {
        fieldErrors.name = "Target names must remain unique within the project.";
      }
      seenNames.add(duplicateKey);
    }

    if (!target.targetPlatform.trim()) {
      fieldErrors.targetPlatform = "Unity target platform is required.";
    }

    if (!target.buildMethod.trim()) {
      fieldErrors.buildMethod = "Build method is required.";
    } else if (!target.buildMethod.includes(".")) {
      fieldErrors.buildMethod =
        "Use a full static method path such as Builder.PerformWindows.";
    }

    if (!target.unityExecutablePath.trim()) {
      fieldErrors.unityExecutablePath = "Unity executable path is required.";
    } else if (validatingTargets[target.id]) {
      fieldErrors.unityExecutablePath =
        "Unity executable validation is still running.";
    } else if (!pathDiagnostics[target.id]) {
      fieldErrors.unityExecutablePath =
        "Unity executable path has not been validated yet.";
    } else if (pathDiagnostics[target.id]?.status !== "ready") {
      fieldErrors.unityExecutablePath =
        pathDiagnostics[target.id]?.message ||
        "Unity executable path is invalid.";
    }

    errors.buildTargets[target.id] = fieldErrors;
  }

  return errors;
}

function hasValidationErrors(errors: RepositoryProjectValidationErrors) {
  return Boolean(
    errors.engineKind ||
      errors.name ||
      errors.repositoryUrl ||
      errors.pollingIntervalSeconds ||
      errors.buildTargetsRoot ||
      Object.values(errors.buildTargets).some((fieldErrors) =>
        hasBuildTargetFieldErrors(fieldErrors),
      ),
  );
}

function hasBuildTargetFieldErrors(
  errors: RepositoryProjectBuildTargetValidationErrors,
) {
  return Boolean(
    errors.name ||
      errors.targetPlatform ||
      errors.buildMethod ||
      errors.unityExecutablePath,
  );
}

function collectInvalidTargetIds(errors: RepositoryProjectValidationErrors) {
  return Object.entries(errors.buildTargets)
    .filter(([, fieldErrors]) => hasBuildTargetFieldErrors(fieldErrors))
    .map(([targetId]) => targetId);
}

function mergeExpandedTargetIds(
  current: Record<string, boolean>,
  targetIds: string[],
) {
  if (targetIds.length === 0) {
    return current;
  }

  const next = { ...current };
  for (const targetId of targetIds) {
    next[targetId] = true;
  }

  return next;
}

function buildRepositoryProjectUpdateInput(
  repositoryId: number,
  draft: RepositoryProjectDraft,
): UpdateRepositoryProjectInput {
  return {
    repository_id: repositoryId,
    name: draft.name.trim(),
    engine_kind: draft.engineKind,
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
    build_targets: draft.buildTargets.map((target) => ({
      build_target_id: target.buildTargetId,
      name: target.name.trim(),
      contract: {
        unity: {
          target_platform: normalizeUnityTargetPlatformValue(
            target.targetPlatform,
          ),
          build_method: target.buildMethod.trim(),
        },
      },
      unity_executable_path: target.unityExecutablePath.trim(),
    })),
  };
}

function isRepositoryProjectDraftChanged(
  repository: RepositoryInspectionEntry,
  draft: RepositoryProjectDraft,
) {
  const persistedDraft = buildRepositoryProjectDraft(
    repository,
    buildRepositoryProjectTargetEditorState(repository).buildTargets,
  );

  return (
    persistedDraft.name.trim() !== draft.name.trim() ||
    persistedDraft.repositoryUrl.trim() !== draft.repositoryUrl.trim() ||
    persistedDraft.defaultBranch.trim() !== draft.defaultBranch.trim() ||
    persistedDraft.artifactsRootOverride.trim() !==
      draft.artifactsRootOverride.trim() ||
    persistedDraft.workspaceRootOverride.trim() !==
      draft.workspaceRootOverride.trim() ||
    persistedDraft.pollingIntervalSeconds !== draft.pollingIntervalSeconds ||
    persistedDraft.enabled !== draft.enabled ||
    !areBuildTargetDraftsEqual(persistedDraft.buildTargets, draft.buildTargets)
  );
}

function areBuildTargetDraftsEqual(
  left: RepositoryProjectBuildTargetDraft[],
  right: RepositoryProjectBuildTargetDraft[],
) {
  if (left.length !== right.length) {
    return false;
  }

  return left.every((target, index) => {
    const candidate = right[index];
    if (!candidate) {
      return false;
    }

    return (
      target.buildTargetId === candidate.buildTargetId &&
      target.name.trim() === candidate.name.trim() &&
      target.targetPlatform.trim() === candidate.targetPlatform.trim() &&
      target.buildMethod.trim() === candidate.buildMethod.trim() &&
      target.unityExecutablePath.trim() ===
        candidate.unityExecutablePath.trim()
    );
  });
}

function formatDiagnosticStatus(status: string) {
  return status.replace(/_/g, " ");
}

function normalizeOptionalDraftValue(value: string) {
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function normalizeRepositoryEngineKind(value: string): RepositoryEngineKind {
  return value === "unity" ? "unity" : "unity";
}

function normalizeUnityTargetPlatformValue(value: string) {
  switch (value.trim().toLocaleLowerCase()) {
    case "windows":
      return "StandaloneWindows64";
    case "linux":
      return "StandaloneLinux64";
    case "macos":
    case "mac":
    case "osx":
      return "StandaloneOSX";
    case "webgl":
      return "WebGL";
    case "android":
      return "Android";
    default:
      return value.trim();
  }
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
