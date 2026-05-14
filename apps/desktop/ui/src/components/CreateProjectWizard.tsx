import {
  startTransition,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";

import { Button } from "./Button";
import { SelectField, TextField } from "./Field";
import { PathPickerField } from "./PathPickerField";
import { Badge, SurfacePanel } from "./Surface";
import { VerticalAccordion } from "./VerticalAccordion";
import {
  createRepositoryProject,
  loadRepositoryInspection,
  validateUnityExecutablePath,
  type CreateRepositoryProjectInput,
  type RepositoryInspectionEntry,
  type UnityExecutableValidation,
} from "../services/projects";

type BuildTargetDraft = {
  id: string;
  name: string;
  platform: string;
  buildMethod: string;
  unityExecutablePath: string;
};

type ProjectDraft = {
  projectKind: "repository" | "local";
  name: string;
  repositoryUrl: string;
  personalAccessToken: string;
  defaultBranch: string;
  pollingIntervalSeconds: string;
  artifactsRootOverride: string;
  workspaceRootOverride: string;
  buildTargets: BuildTargetDraft[];
};

type WizardStepKey = "identity" | "access" | "targets" | "paths" | "review";

type TargetFieldErrors = {
  name?: string;
  platform?: string;
  buildMethod?: string;
  unityExecutablePath?: string;
};

type TargetStepErrors = {
  root?: string;
  targets: Record<string, TargetFieldErrors>;
};

type PathStepErrors = {
  artifactsRootOverride?: string;
  workspaceRootOverride?: string;
};

type ProjectPathFieldName = "artifactsRootOverride" | "workspaceRootOverride";

type CreateProjectWizardProps = {
  onCreated: (repositoryId: number) => void;
};

type ValidationTimerMap = Record<string, number | undefined>;

const WIZARD_STEPS: Array<{
  key: WizardStepKey;
  label: string;
  description: string;
}> = [
  {
    key: "identity",
    label: "Identity",
    description:
      "Name the project first, then choose how HUP should register it.",
  },
  {
    key: "access",
    label: "Repository",
    description: "Declare the remote, branch hints, and polling cadence.",
  },
  {
    key: "targets",
    label: "Build Targets",
    description: "Compose the host-native Unity targets that HUP will execute.",
  },
  {
    key: "paths",
    label: "Paths",
    description:
      "Choose optional repository-specific artifact and workspace paths.",
  },
  {
    key: "review",
    label: "Review",
    description: "Review the resulting project and finalize the registration.",
  },
];

const PROJECT_KIND_OPTIONS = [
  { label: "Repository project", value: "repository" },
  {
    label: "Local workspace project",
    value: "local",
    disabled: true,
  },
] as const;

const PLATFORM_OPTIONS = [
  { label: "Select a platform", value: "" },
  { label: "Windows", value: "windows" },
  { label: "Linux", value: "linux" },
  { label: "macOS", value: "macos" },
  { label: "WebGL", value: "webgl" },
  { label: "Android", value: "android" },
] as const;

const EMPTY_VALIDATION_ATTEMPTS: Record<WizardStepKey, boolean> = {
  identity: false,
  access: false,
  targets: false,
  paths: false,
  review: false,
};

export function CreateProjectWizard({ onCreated }: CreateProjectWizardProps) {
  const [draft, setDraft] = useState<ProjectDraft>(() => ({
    projectKind: "repository",
    name: "",
    repositoryUrl: "",
    personalAccessToken: "",
    defaultBranch: "main",
    pollingIntervalSeconds: "300",
    artifactsRootOverride: "",
    workspaceRootOverride: "",
    buildTargets: [createEmptyBuildTargetDraft(1)],
  }));
  const [currentStepIndex, setCurrentStepIndex] = useState(0);
  const [attemptedSteps, setAttemptedSteps] = useState(
    EMPTY_VALIDATION_ATTEMPTS,
  );
  const [touchedFields, setTouchedFields] = useState<Record<string, boolean>>(
    {},
  );
  const [repositoryInventory, setRepositoryInventory] = useState<
    RepositoryInspectionEntry[]
  >([]);
  const [isLoadingRepositoryInventory, setIsLoadingRepositoryInventory] =
    useState(true);
  const [inventoryError, setInventoryError] = useState<string | null>(null);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [pathDiagnostics, setPathDiagnostics] = useState<
    Record<string, UnityExecutableValidation | null>
  >({});
  const [validatingTargets, setValidatingTargets] = useState<
    Record<string, boolean>
  >({});
  const [expandedTargetIds, setExpandedTargetIds] = useState<
    Record<string, boolean>
  >({
    "target-1": true,
  });
  const nextBuildTargetIdRef = useRef(2);
  const validationTimersRef = useRef<ValidationTimerMap>({});
  const validationTokenRef = useRef<Record<string, number>>({});

  const currentStep = WIZARD_STEPS[currentStepIndex];
  const showPreviousAction = currentStepIndex > 0;
  const showNextAction = currentStep.key !== "review";
  const identityErrors = validateIdentityStep(draft, repositoryInventory);
  const accessErrors = validateAccessStep(draft, repositoryInventory);
  const targetErrors = validateTargetsStep(
    draft,
    pathDiagnostics,
    validatingTargets,
  );
  const pathErrors = validatePathStep(draft);

  const loadRepositoryInventoryEffect = useEffectEvent(async () => {
    setIsLoadingRepositoryInventory(true);

    try {
      const inspection = await loadRepositoryInspection();

      startTransition(() => {
        setRepositoryInventory(inspection.repositories);
        setInventoryError(null);
        setIsLoadingRepositoryInventory(false);
      });
    } catch (error) {
      startTransition(() => {
        setInventoryError(buildProjectErrorMessage(error));
        setIsLoadingRepositoryInventory(false);
      });
    }
  });

  useEffect(() => {
    void loadRepositoryInventoryEffect();

    return () => {
      for (const timerId of Object.values(validationTimersRef.current)) {
        if (timerId !== undefined) {
          window.clearTimeout(timerId);
        }
      }
    };
  }, []);

  const markFieldTouched = useEffectEvent((fieldKey: string) => {
    startTransition(() => {
      setTouchedFields((current) => ({
        ...current,
        [fieldKey]: true,
      }));
    });
  });

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
        } catch (error) {
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
                message: buildProjectErrorMessage(error),
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

  const updateBuildTarget = useEffectEvent(
    (targetId: string, patch: Partial<BuildTargetDraft>) => {
      startTransition(() => {
        setDraft((current) => ({
          ...current,
          buildTargets: current.buildTargets.map((target) =>
            target.id === targetId ? { ...target, ...patch } : target,
          ),
        }));
      });
    },
  );

  const handlePathPickerError = useEffectEvent((error: unknown) => {
    startTransition(() => {
      setSubmitError(buildProjectErrorMessage(error));
    });
  });

  const handlePickUnityExecutablePath = useEffectEvent(
    (targetId: string, selectedPath: string) => {
      updateBuildTarget(targetId, { unityExecutablePath: selectedPath });
      markFieldTouched(buildTargetFieldKey(targetId, "unityExecutablePath"));
      scheduleUnityExecutableValidation(targetId, selectedPath, 0);
    },
  );

  const handleProjectPathPicked = useEffectEvent(
    (fieldName: ProjectPathFieldName, selectedPath: string) => {
      startTransition(() => {
        setDraft((current) => ({
          ...current,
          [fieldName]: selectedPath,
        }));
      });
      markFieldTouched(fieldName);
    },
  );

  const handleProjectPathCleared = useEffectEvent(
    (fieldName: ProjectPathFieldName) => {
      startTransition(() => {
        setDraft((current) => ({
          ...current,
          [fieldName]: "",
        }));
      });
      markFieldTouched(fieldName);
    },
  );

  const handleAddBuildTarget = useEffectEvent(() => {
    const nextTarget = createEmptyBuildTargetDraft(
      nextBuildTargetIdRef.current,
    );
    nextBuildTargetIdRef.current += 1;

    startTransition(() => {
      setDraft((current) => ({
        ...current,
        buildTargets: [...current.buildTargets, nextTarget],
      }));
      setExpandedTargetIds((current) => ({
        ...current,
        [nextTarget.id]: true,
      }));
    });
  });

  const handleRemoveBuildTarget = useEffectEvent((targetId: string) => {
    const existingTimerId = validationTimersRef.current[targetId];
    if (existingTimerId !== undefined) {
      window.clearTimeout(existingTimerId);
    }

    startTransition(() => {
      setDraft((current) => ({
        ...current,
        buildTargets: current.buildTargets.filter(
          (target) => target.id !== targetId,
        ),
      }));
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

  const handleAdvanceStep = useEffectEvent(() => {
    if (currentStep.key === "identity") {
      if (hasIdentityErrors(identityErrors) || isLoadingRepositoryInventory) {
        startTransition(() => {
          setAttemptedSteps((current) => ({
            ...current,
            identity: true,
          }));
        });
        return;
      }
    }

    if (currentStep.key === "access") {
      if (hasAccessErrors(accessErrors) || isLoadingRepositoryInventory) {
        startTransition(() => {
          setAttemptedSteps((current) => ({
            ...current,
            access: true,
          }));
        });
        return;
      }
    }

    if (currentStep.key === "targets") {
      if (hasTargetErrors(targetErrors)) {
        const invalidTargetIds = collectInvalidTargetIds(targetErrors);
        startTransition(() => {
          setAttemptedSteps((current) => ({
            ...current,
            targets: true,
          }));
          setExpandedTargetIds((current) =>
            mergeExpandedTargetIds(current, invalidTargetIds),
          );
        });
        return;
      }
    }

    if (currentStep.key === "paths") {
      if (hasPathErrors(pathErrors)) {
        startTransition(() => {
          setAttemptedSteps((current) => ({
            ...current,
            paths: true,
          }));
        });
        return;
      }
    }

    startTransition(() => {
      setCurrentStepIndex((current) =>
        Math.min(current + 1, WIZARD_STEPS.length - 1),
      );
    });
  });

  const handleRetreatStep = useEffectEvent(() => {
    startTransition(() => {
      setCurrentStepIndex((current) => Math.max(current - 1, 0));
    });
  });

  const handleSubmitProject = useEffectEvent(async () => {
    const firstInvalidStep = findFirstInvalidStep({
      identityErrors,
      accessErrors,
      targetErrors,
      pathErrors,
      isLoadingRepositoryInventory,
    });
    if (firstInvalidStep) {
      const invalidTargetIds =
        firstInvalidStep === "targets"
          ? collectInvalidTargetIds(targetErrors)
          : [];

      startTransition(() => {
        setAttemptedSteps((current) => ({
          ...current,
          [firstInvalidStep]: true,
        }));
        if (firstInvalidStep === "targets") {
          setExpandedTargetIds((current) =>
            mergeExpandedTargetIds(current, invalidTargetIds),
          );
        }
        setCurrentStepIndex(indexOfWizardStep(firstInvalidStep));
      });
      return;
    }

    setIsSubmitting(true);
    setSubmitError(null);

    try {
      const created = await createRepositoryProject(
        buildCreateProjectInput(draft),
      );

      startTransition(() => {
        setIsSubmitting(false);
        onCreated(created.repository_id);
      });
    } catch (error) {
      startTransition(() => {
        setIsSubmitting(false);
        setSubmitError(buildProjectErrorMessage(error));
      });
    }
  });

  return (
    <div className="wizard-shell">
      <div className="wizard-stepper" aria-label="Create project progress">
        {WIZARD_STEPS.map((step, index) => (
          <button
            className={joinClassNames(
              "wizard-stepper__item",
              index === currentStepIndex && "wizard-stepper__item--current",
              index < currentStepIndex && "wizard-stepper__item--complete",
            )}
            disabled={index > currentStepIndex}
            key={step.key}
            onClick={() => {
              if (index <= currentStepIndex) {
                startTransition(() => {
                  setCurrentStepIndex(index);
                });
              }
            }}
            type="button"
          >
            <span className="wizard-stepper__index">{index + 1}</span>
            <span className="wizard-stepper__label">{step.label}</span>
          </button>
        ))}
      </div>

      {inventoryError ? (
        <p className="feed-banner feed-banner--error">{inventoryError}</p>
      ) : null}

      {submitError ? (
        <p className="feed-banner feed-banner--error">{submitError}</p>
      ) : null}

      <SurfacePanel
        className="ui-panel--wizard-stage"
        description={currentStep.description}
        eyebrow="Create Project"
        title={currentStep.label}
      >
        {currentStep.key === "identity" ? (
          <div className="wizard-form-grid">
            <TextField
              error={
                shouldShowFieldError(
                  attemptedSteps.identity,
                  touchedFields,
                  "name",
                )
                  ? identityErrors.name
                  : undefined
              }
              label="Project name"
              onBlur={() => markFieldTouched("name")}
              onChange={(event) => {
                const nextValue = event.currentTarget.value;
                startTransition(() => {
                  setDraft((current) => ({
                    ...current,
                    name: nextValue,
                  }));
                });
                markFieldTouched("name");
              }}
              placeholder="Red Horizon"
              value={draft.name}
            />

            <SelectField
              error={
                shouldShowFieldError(
                  attemptedSteps.identity,
                  touchedFields,
                  "projectKind",
                )
                  ? identityErrors.projectKind
                  : undefined
              }
              label="Project kind"
              onBlur={() => markFieldTouched("projectKind")}
              onChange={(event) => {
                const projectKind = event.currentTarget
                  .value as ProjectDraft["projectKind"];
                startTransition(() => {
                  setDraft((current) => ({
                    ...current,
                    projectKind,
                  }));
                });
                markFieldTouched("projectKind");
              }}
              options={PROJECT_KIND_OPTIONS}
              value={draft.projectKind}
            />

            <div className="wizard-callout wizard-callout--compact">
              <p className="wizard-callout__copy">
                Repository projects let HUP poll a remote Git repository on a
                fixed cadence and queue automation when a new release tag
                appears. A local workspace project would use files that are
                already present on this machine and would not depend on
                repository polling.
              </p>
            </div>
          </div>
        ) : null}

        {currentStep.key === "access" ? (
          <div className="wizard-form-grid">
            <TextField
              error={
                shouldShowFieldError(
                  attemptedSteps.access,
                  touchedFields,
                  "repositoryUrl",
                )
                  ? accessErrors.repositoryUrl
                  : undefined
              }
              hint="Use the HTTPS remote that HUP will poll and clone."
              label="Repository URL"
              leadingIcon="server"
              onBlur={() => markFieldTouched("repositoryUrl")}
              onChange={(event) => {
                const nextValue = event.currentTarget.value;
                startTransition(() => {
                  setDraft((current) => ({
                    ...current,
                    repositoryUrl: nextValue,
                  }));
                });
                markFieldTouched("repositoryUrl");
              }}
              placeholder="https://github.com/org/project.git"
              value={draft.repositoryUrl}
            />
            <TextField
              error={
                shouldShowFieldError(
                  attemptedSteps.access,
                  touchedFields,
                  "defaultBranch",
                )
                  ? accessErrors.defaultBranch
                  : undefined
              }
              hint="Optional. HUP uses this when branch-aware operations need a default ref."
              label="Default branch"
              onBlur={() => markFieldTouched("defaultBranch")}
              onChange={(event) => {
                const nextValue = event.currentTarget.value;
                startTransition(() => {
                  setDraft((current) => ({
                    ...current,
                    defaultBranch: nextValue,
                  }));
                });
                markFieldTouched("defaultBranch");
              }}
              placeholder="main"
              value={draft.defaultBranch}
            />
            <TextField
              error={
                shouldShowFieldError(
                  attemptedSteps.access,
                  touchedFields,
                  "pollingIntervalSeconds",
                )
                  ? accessErrors.pollingIntervalSeconds
                  : undefined
              }
              hint="Polling stays operator-visible. The runtime requires at least 5 seconds."
              label="Polling interval (seconds)"
              min={5}
              onBlur={() => markFieldTouched("pollingIntervalSeconds")}
              onChange={(event) => {
                const nextValue = event.currentTarget.value;
                startTransition(() => {
                  setDraft((current) => ({
                    ...current,
                    pollingIntervalSeconds: nextValue,
                  }));
                });
                markFieldTouched("pollingIntervalSeconds");
              }}
              step={5}
              type="number"
              value={draft.pollingIntervalSeconds}
            />
            <TextField
              error={
                shouldShowFieldError(
                  attemptedSteps.access,
                  touchedFields,
                  "personalAccessToken",
                )
                  ? accessErrors.personalAccessToken
                  : undefined
              }
              hint="Optional for public repositories. When set, the token is written to the host keyring and only a reference stays in SQLite."
              label="Personal access token"
              onBlur={() => markFieldTouched("personalAccessToken")}
              onChange={(event) => {
                const nextValue = event.currentTarget.value;
                startTransition(() => {
                  setDraft((current) => ({
                    ...current,
                    personalAccessToken: nextValue,
                  }));
                });
                markFieldTouched("personalAccessToken");
              }}
              placeholder="Leave blank for public repositories"
              type="password"
              value={draft.personalAccessToken}
            />
          </div>
        ) : null}

        {currentStep.key === "targets" ? (
          <div className="wizard-targets-shell">
            {shouldShowStepError(attemptedSteps.targets) &&
            targetErrors.root ? (
              <p className="feed-banner feed-banner--error">
                {targetErrors.root}
              </p>
            ) : null}

            {draft.buildTargets.map((target, index) => {
              const diagnostics = pathDiagnostics[target.id];
              const fieldErrors = targetErrors.targets[target.id] ?? {};

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
                          disabled={draft.buildTargets.length === 1}
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
                      error={
                        shouldShowFieldError(
                          attemptedSteps.targets,
                          touchedFields,
                          buildTargetFieldKey(target.id, "name"),
                        )
                          ? fieldErrors.name
                          : undefined
                      }
                      hint="Keep the target name stable. It becomes part of the artifact file name."
                      label="Target name"
                      onBlur={() =>
                        markFieldTouched(buildTargetFieldKey(target.id, "name"))
                      }
                      onChange={(event) => {
                        updateBuildTarget(target.id, {
                          name: event.currentTarget.value,
                        });
                        markFieldTouched(
                          buildTargetFieldKey(target.id, "name"),
                        );
                      }}
                      placeholder="Windows"
                      value={target.name}
                    />
                    <SelectField
                      error={
                        shouldShowFieldError(
                          attemptedSteps.targets,
                          touchedFields,
                          buildTargetFieldKey(target.id, "platform"),
                        )
                          ? fieldErrors.platform
                          : undefined
                      }
                      hint="The runtime maps this directly to Unity BuildTarget semantics."
                      label="Platform"
                      onBlur={() =>
                        markFieldTouched(
                          buildTargetFieldKey(target.id, "platform"),
                        )
                      }
                      onChange={(event) => {
                        updateBuildTarget(target.id, {
                          platform: event.currentTarget.value,
                        });
                        markFieldTouched(
                          buildTargetFieldKey(target.id, "platform"),
                        );
                      }}
                      options={PLATFORM_OPTIONS}
                      value={target.platform}
                    />
                    <TextField
                      error={
                        shouldShowFieldError(
                          attemptedSteps.targets,
                          touchedFields,
                          buildTargetFieldKey(target.id, "buildMethod"),
                        )
                          ? fieldErrors.buildMethod
                          : undefined
                      }
                      hint="Point this at a real static Unity method, for example Builder.PerformWindows."
                      label="Build method"
                      onBlur={() =>
                        markFieldTouched(
                          buildTargetFieldKey(target.id, "buildMethod"),
                        )
                      }
                      onChange={(event) => {
                        updateBuildTarget(target.id, {
                          buildMethod: event.currentTarget.value,
                        });
                        markFieldTouched(
                          buildTargetFieldKey(target.id, "buildMethod"),
                        );
                      }}
                      placeholder="Builder.PerformWindows"
                      value={target.buildMethod}
                    />
                    <PathPickerField
                      buttonLabel="Choose Unity executable"
                      disabled={isSubmitting}
                      dialogTitle="Select Unity Editor executable"
                      error={
                        shouldShowFieldError(
                          attemptedSteps.targets,
                          touchedFields,
                          buildTargetFieldKey(target.id, "unityExecutablePath"),
                        )
                          ? fieldErrors.unityExecutablePath
                          : undefined
                      }
                      filters={[
                        {
                          name: "Unity Editor",
                          extensions: ["exe", "app"],
                        },
                      ]}
                      hint="Select the host-local Unity Editor executable that should run this target."
                      label="Unity executable"
                      onError={handlePathPickerError}
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

            <div className="wizard-targets-shell__footer">
              <Button
                leadingIcon="plus"
                onClick={handleAddBuildTarget}
                size="sm"
                variant="secondary"
              >
                Add target
              </Button>
            </div>
          </div>
        ) : null}

        {currentStep.key === "paths" ? (
          <div className="wizard-form-grid">
            <PathPickerField
              buttonLabel="Choose artifacts root"
              clearable
              disabled={isSubmitting}
              dialogTitle="Select artifacts root directory"
              error={
                shouldShowFieldError(
                  attemptedSteps.paths,
                  touchedFields,
                  "artifactsRootOverride",
                )
                  ? pathErrors.artifactsRootOverride
                  : undefined
              }
              hint="Optional. Override the artifact root for this repository only."
              label="Artifacts root override"
              onClear={() => handleProjectPathCleared("artifactsRootOverride")}
              onError={handlePathPickerError}
              onPathPicked={(selectedPath) =>
                handleProjectPathPicked("artifactsRootOverride", selectedPath)
              }
              pickerKind="directory"
              placeholder="C:/builds/red-horizon"
              value={draft.artifactsRootOverride}
            />

            <PathPickerField
              buttonLabel="Choose workspace root"
              clearable
              disabled={isSubmitting}
              dialogTitle="Select managed workspace root directory"
              error={
                shouldShowFieldError(
                  attemptedSteps.paths,
                  touchedFields,
                  "workspaceRootOverride",
                )
                  ? pathErrors.workspaceRootOverride
                  : undefined
              }
              hint="Optional. Override the managed checkout root for this repository only."
              label="Workspace root override"
              onClear={() => handleProjectPathCleared("workspaceRootOverride")}
              onError={handlePathPickerError}
              onPathPicked={(selectedPath) =>
                handleProjectPathPicked("workspaceRootOverride", selectedPath)
              }
              pickerKind="directory"
              placeholder="C:/workspaces/red-horizon"
              value={draft.workspaceRootOverride}
            />
          </div>
        ) : null}

        {currentStep.key === "review" ? (
          <div className="wizard-review-shell">
            <section className="wizard-summary-panel">
              <div>
                <p className="wizard-summary-panel__eyebrow">Project</p>
                <h3 className="wizard-summary-panel__title">
                  {draft.name.trim() || "Unnamed project"}
                </h3>
                <p className="wizard-summary-panel__copy">
                  {draft.repositoryUrl.trim() || "Repository URL not set yet."}
                </p>
              </div>

              <div className="wizard-summary-panel__stats">
                <Badge tone="neutral">
                  Poll every {draft.pollingIntervalSeconds.trim() || "0"}s
                </Badge>
                <Badge tone="muted">
                  {draft.personalAccessToken.trim()
                    ? "PAT will be stored in host keyring"
                    : "Public repository mode"}
                </Badge>
                <Badge tone="muted">
                  {draft.buildTargets.length} target
                  {draft.buildTargets.length === 1 ? "" : "s"}
                </Badge>
              </div>

              <div>
                <p className="wizard-summary-panel__eyebrow">Build targets</p>
                <div className="wizard-summary-list">
                  {draft.buildTargets.map((target) => (
                    <div className="wizard-summary-list__item" key={target.id}>
                      <div className="wizard-summary-list__title-row">
                        <strong>
                          {target.name.trim() || "Unnamed target"}
                        </strong>
                        <Badge tone="neutral">
                          {target.platform || "platform pending"}
                        </Badge>
                      </div>
                      <p className="wizard-summary-list__copy">
                        {target.buildMethod.trim() || "Build method pending"}
                      </p>
                      <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
                        {target.unityExecutablePath.trim() ||
                          "Unity executable pending"}
                      </p>
                    </div>
                  ))}
                </div>
              </div>

              <div>
                <p className="wizard-summary-panel__eyebrow">Paths</p>
                <div className="wizard-summary-list">
                  <div className="wizard-summary-list__item">
                    <div className="wizard-summary-list__title-row">
                      <strong>Artifacts root</strong>
                      <Badge tone="muted">
                        {draft.artifactsRootOverride.trim()
                          ? "override"
                          : "default"}
                      </Badge>
                    </div>
                    <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
                      {draft.artifactsRootOverride.trim() ||
                        "Use the runtime default artifact root."}
                    </p>
                  </div>

                  <div className="wizard-summary-list__item">
                    <div className="wizard-summary-list__title-row">
                      <strong>Workspace root</strong>
                      <Badge tone="muted">
                        {draft.workspaceRootOverride.trim()
                          ? "override"
                          : "default"}
                      </Badge>
                    </div>
                    <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
                      {draft.workspaceRootOverride.trim() ||
                        "Use the runtime default managed checkout root."}
                    </p>
                  </div>
                </div>
              </div>
            </section>
          </div>
        ) : null}
      </SurfacePanel>

      <footer className="wizard-footer">
        <div className="wizard-footer__slot wizard-footer__slot--start">
          {showPreviousAction ? (
            <Button
              disabled={isSubmitting}
              onClick={handleRetreatStep}
              size="sm"
              variant="ghost"
            >
              Previous
            </Button>
          ) : null}
        </div>

        <div className="wizard-footer__slot wizard-footer__slot--end">
          {currentStep.key === "review" ? (
            <Button
              disabled={isSubmitting}
              onClick={() => void handleSubmitProject()}
              size="sm"
              variant="primary"
            >
              {isSubmitting ? "Creating..." : "Create project"}
            </Button>
          ) : null}

          {showNextAction ? (
            <Button
              disabled={isSubmitting || isLoadingRepositoryInventory}
              onClick={handleAdvanceStep}
              size="sm"
              variant="primary"
            >
              Next
            </Button>
          ) : null}
        </div>
      </footer>
    </div>
  );
}

function createEmptyBuildTargetDraft(index: number): BuildTargetDraft {
  return {
    id: `target-${index}`,
    name: "",
    platform: "",
    buildMethod: "",
    unityExecutablePath: "",
  };
}

function validateIdentityStep(
  draft: ProjectDraft,
  repositoryInventory: RepositoryInspectionEntry[],
) {
  const errors: { name?: string; projectKind?: string } = {};
  const normalizedName = draft.name.trim();
  if (!normalizedName) {
    errors.name = "Project name is required.";
  } else if (
    repositoryInventory.some(
      (repository) =>
        repository.repository_name.trim().toLocaleLowerCase() ===
        normalizedName.toLocaleLowerCase(),
    )
  ) {
    errors.name = "Another repository project already uses this name.";
  }

  if (draft.projectKind !== "repository") {
    errors.projectKind =
      "Only repository projects are available in this release.";
  }

  return errors;
}

function validateAccessStep(
  draft: ProjectDraft,
  repositoryInventory: RepositoryInspectionEntry[],
) {
  const errors: {
    repositoryUrl?: string;
    defaultBranch?: string;
    pollingIntervalSeconds?: string;
    personalAccessToken?: string;
  } = {};
  const normalizedUrl = draft.repositoryUrl.trim();
  if (!normalizedUrl) {
    errors.repositoryUrl = "Repository URL is required.";
  } else if (
    !(
      normalizedUrl.startsWith("https://") ||
      normalizedUrl.startsWith("http://")
    )
  ) {
    errors.repositoryUrl = "Repository URL must use http:// or https://.";
  } else if (
    repositoryInventory.some(
      (repository) =>
        repository.repo_url.trim().toLocaleLowerCase() ===
        normalizedUrl.toLocaleLowerCase(),
    )
  ) {
    errors.repositoryUrl = "This remote is already registered in HUP.";
  }

  const normalizedBranch = draft.defaultBranch.trim();
  if (normalizedBranch && /\s/.test(normalizedBranch)) {
    errors.defaultBranch = "Default branch must not contain whitespace.";
  }

  const pollingInterval = Number(draft.pollingIntervalSeconds.trim());
  if (!Number.isInteger(pollingInterval)) {
    errors.pollingIntervalSeconds = "Polling interval must be a whole number.";
  } else if (pollingInterval < 5) {
    errors.pollingIntervalSeconds =
      "Polling interval must be at least 5 seconds.";
  }

  const personalAccessToken = draft.personalAccessToken.trim();
  if (personalAccessToken && /\s/.test(personalAccessToken)) {
    errors.personalAccessToken =
      "Personal access token must not contain whitespace.";
  }

  return errors;
}

function validateTargetsStep(
  draft: ProjectDraft,
  pathDiagnostics: Record<string, UnityExecutableValidation | null>,
  validatingTargets: Record<string, boolean>,
): TargetStepErrors {
  const errors: TargetStepErrors = {
    targets: {},
  };
  if (draft.buildTargets.length === 0) {
    errors.root = "At least one build target is required.";
    return errors;
  }

  const seenNames = new Set<string>();
  for (const target of draft.buildTargets) {
    const fieldErrors: TargetFieldErrors = {};
    const normalizedName = target.name.trim();
    if (!normalizedName) {
      fieldErrors.name = "Target name is required.";
    } else {
      const duplicateKey = normalizedName.toLocaleLowerCase();
      if (seenNames.has(duplicateKey)) {
        fieldErrors.name =
          "Target names must remain unique within the project.";
      }
      seenNames.add(duplicateKey);
    }

    if (!target.platform.trim()) {
      fieldErrors.platform = "Platform selection is required.";
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

    errors.targets[target.id] = fieldErrors;
  }

  return errors;
}

function validatePathStep(draft: ProjectDraft): PathStepErrors {
  const errors: PathStepErrors = {};
  const normalizedArtifactsRoot = draft.artifactsRootOverride.trim();
  const normalizedWorkspaceRoot = draft.workspaceRootOverride.trim();
  if (
    normalizedArtifactsRoot &&
    !looksLikeAbsolutePath(normalizedArtifactsRoot)
  ) {
    errors.artifactsRootOverride =
      "Artifacts root override must be an absolute path.";
  }
  if (
    normalizedWorkspaceRoot &&
    !looksLikeAbsolutePath(normalizedWorkspaceRoot)
  ) {
    errors.workspaceRootOverride =
      "Workspace root override must be an absolute path.";
  }

  return errors;
}

function hasIdentityErrors(errors: ReturnType<typeof validateIdentityStep>) {
  return Boolean(errors.name || errors.projectKind);
}

function hasAccessErrors(errors: ReturnType<typeof validateAccessStep>) {
  return Boolean(
    errors.repositoryUrl ||
    errors.defaultBranch ||
    errors.pollingIntervalSeconds ||
    errors.personalAccessToken,
  );
}

function hasTargetErrors(errors: TargetStepErrors) {
  return Boolean(
    errors.root ||
    Object.values(errors.targets).some((fieldErrors) =>
      Object.values(fieldErrors).some(Boolean),
    ),
  );
}

function hasTargetFieldErrors(errors: TargetFieldErrors) {
  return Boolean(
    errors.name ||
    errors.platform ||
    errors.buildMethod ||
    errors.unityExecutablePath,
  );
}

function collectInvalidTargetIds(errors: TargetStepErrors) {
  return Object.entries(errors.targets)
    .filter(([, fieldErrors]) => hasTargetFieldErrors(fieldErrors))
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

function hasPathErrors(errors: PathStepErrors) {
  return Boolean(errors.artifactsRootOverride || errors.workspaceRootOverride);
}

function findFirstInvalidStep(input: {
  identityErrors: ReturnType<typeof validateIdentityStep>;
  accessErrors: ReturnType<typeof validateAccessStep>;
  targetErrors: TargetStepErrors;
  pathErrors: PathStepErrors;
  isLoadingRepositoryInventory: boolean;
}): WizardStepKey | null {
  if (
    input.isLoadingRepositoryInventory ||
    hasIdentityErrors(input.identityErrors)
  ) {
    return "identity";
  }
  if (
    input.isLoadingRepositoryInventory ||
    hasAccessErrors(input.accessErrors)
  ) {
    return "access";
  }
  if (hasTargetErrors(input.targetErrors)) {
    return "targets";
  }
  if (hasPathErrors(input.pathErrors)) {
    return "paths";
  }

  return null;
}

function buildCreateProjectInput(
  draft: ProjectDraft,
): CreateRepositoryProjectInput {
  return {
    name: draft.name.trim(),
    repository_url: draft.repositoryUrl.trim(),
    personal_access_token: optionalTrimmedString(draft.personalAccessToken),
    default_branch: optionalTrimmedString(draft.defaultBranch),
    artifacts_root_override: optionalTrimmedString(draft.artifactsRootOverride),
    workspace_root_override: optionalTrimmedString(draft.workspaceRootOverride),
    polling_interval_seconds: Number(draft.pollingIntervalSeconds.trim()),
    build_targets: draft.buildTargets.map((target) => ({
      name: target.name.trim(),
      platform: target.platform.trim(),
      build_method: target.buildMethod.trim(),
      unity_executable_path: target.unityExecutablePath.trim(),
    })),
  };
}

function shouldShowFieldError(
  attemptedStep: boolean,
  touchedFields: Record<string, boolean>,
  fieldKey: string,
) {
  return attemptedStep || Boolean(touchedFields[fieldKey]);
}

function shouldShowStepError(attemptedStep: boolean) {
  return attemptedStep;
}

function buildTargetFieldKey(
  targetId: string,
  fieldName: keyof Omit<BuildTargetDraft, "id">,
) {
  return `${targetId}:${fieldName}`;
}

function indexOfWizardStep(stepKey: WizardStepKey) {
  return WIZARD_STEPS.findIndex((step) => step.key === stepKey);
}

function optionalTrimmedString(value: string) {
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function looksLikeAbsolutePath(value: string) {
  return (
    /^[a-zA-Z]:[\\/]/.test(value) ||
    value.startsWith("/") ||
    value.startsWith("\\\\")
  );
}

function formatDiagnosticStatus(status: string) {
  switch (status) {
    case "ready":
      return "ready";
    case "missing_executable":
      return "missing";
    case "invalid_path":
      return "invalid";
    case "validation_failed":
      return "failed";
    default:
      return status.replace(/_/g, " ");
  }
}

function buildProjectErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return "The desktop shell could not complete the project operation.";
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
