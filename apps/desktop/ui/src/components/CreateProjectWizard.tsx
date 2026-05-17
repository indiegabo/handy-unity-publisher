import {
  startTransition,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";

import { Button, IconButton } from "./Button";
import { RepositoryCredentialComposer } from "./RepositoryCredentialComposer";
import {
  PublishDestinationsEditor,
  buildCreateProjectPublishTargetsInput,
  buildPublishDestinationReviewSummary,
  collectBuildTargetBindingImpact,
  hasPublishDestinationValidationErrors,
  listUnboundBuildTargetNames,
  removeBuildTargetBindings,
  type ProjectBuildTargetReference,
  type PublishDestinationDraft,
  validatePublishDestinationDrafts,
} from "./PublishDestinationsEditor";
import { SelectField, TextField, type SelectOption } from "./Field";
import { PathPickerField } from "./PathPickerField";
import { RepositoryEngineField } from "./RepositoryEngineField";
import {
  Badge,
  FocusPageFrame,
  MetaItem,
  MetaRow,
  SurfacePanel,
} from "./Surface";
import { VerticalAccordion } from "./VerticalAccordion";
import {
  createRepositoryProject,
  detectRepositoryProvider,
  loadSecretSettings,
  loadRepositoryInspection,
  saveSecretCredential,
  validateUnityExecutablePath,
  type CreateRepositoryProjectInput,
  type RepositoryAccessAssessment,
  type RepositoryEngineKind,
  type RepositoryProviderDetection,
  type SaveSecretCredentialInput,
  type RepositoryInspectionEntry,
  type SecretCredentialSetting,
  type UnityExecutableValidation,
} from "../services/projects";
import {
  loadAuthProviders,
  loginWithGithubAuth,
  type AuthProviderStatus,
} from "../services/auth";

type BuildTargetDraft = {
  id: string;
  name: string;
  targetPlatform: string;
  buildMethod: string;
  unityExecutablePath: string;
};

type ProjectDraft = {
  projectKind: "repository" | "local";
  engineKind: RepositoryEngineKind;
  name: string;
  repositoryUrl: string;
  repositoryVisibility: "public" | "private";
  defaultBranch: string;
  pollingIntervalSeconds: string;
  artifactsRootOverride: string;
  workspaceRootOverride: string;
  buildTargets: BuildTargetDraft[];
  publishDestinations: PublishDestinationDraft[];
};

type WizardStepKey =
  | "identity"
  | "access"
  | "targets"
  | "publish"
  | "paths"
  | "review";

type TargetFieldErrors = {
  name?: string;
  targetPlatform?: string;
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
  onManageAuth?: () => void;
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
      "Name the project first, then choose how HGP should register it.",
  },
  {
    key: "access",
    label: "Repository",
    description: "Declare the remote, branch hints, and polling cadence.",
  },
  {
    key: "targets",
    label: "Build Targets",
    description: "Compose the host-native Unity targets that HGP will execute.",
  },
  {
    key: "publish",
    label: "Publish Destinations",
    description:
      "Bind build targets to publish destinations and validate destination-specific policy before save.",
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

const REPOSITORY_VISIBILITY_OPTIONS = [
  { label: "Public", value: "public" },
  { label: "Private", value: "private" },
] as const;

const PLATFORM_OPTIONS = [
  { label: "Select a Unity target", value: "" },
  { label: "Windows", value: "StandaloneWindows64" },
  { label: "Linux", value: "StandaloneLinux64" },
  { label: "macOS", value: "StandaloneOSX" },
  { label: "WebGL", value: "WebGL" },
  { label: "Android", value: "Android" },
] as const;

const EMPTY_VALIDATION_ATTEMPTS: Record<WizardStepKey, boolean> = {
  identity: false,
  access: false,
  targets: false,
  publish: false,
  paths: false,
  review: false,
};

export function CreateProjectWizard({
  onCreated,
  onManageAuth,
}: CreateProjectWizardProps) {
  const [draft, setDraft] = useState<ProjectDraft>(() => ({
    projectKind: "repository",
    engineKind: "unity",
    name: "",
    repositoryUrl: "",
    repositoryVisibility: "public",
    defaultBranch: "main",
    pollingIntervalSeconds: "300",
    artifactsRootOverride: "",
    workspaceRootOverride: "",
    buildTargets: [createEmptyBuildTargetDraft(1)],
    publishDestinations: [],
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
  const [githubAuthProvider, setGithubAuthProvider] =
    useState<AuthProviderStatus | null>(null);
  const [isLoadingAuthProviders, setIsLoadingAuthProviders] = useState(true);
  const [authProviderError, setAuthProviderError] = useState<string | null>(
    null,
  );
  const [repositoryCredentials, setRepositoryCredentials] = useState<
    SecretCredentialSetting[]
  >([]);
  const [publishCredentials, setPublishCredentials] = useState<
    SecretCredentialSetting[]
  >([]);
  const [isLoadingRepositoryCredentials, setIsLoadingRepositoryCredentials] =
    useState(true);
  const [repositoryCredentialsError, setRepositoryCredentialsError] = useState<
    string | null
  >(null);
  const [repositoryAccessAssessment, setRepositoryAccessAssessment] =
    useState<RepositoryAccessAssessment | null>(null);
  const [isAssessingRepositoryAccess, setIsAssessingRepositoryAccess] =
    useState(false);
  const [repositoryAccessError, setRepositoryAccessError] = useState<
    string | null
  >(null);
  const [repositoryCredentialId, setRepositoryCredentialId] = useState<
    number | null
  >(null);
  const [repositoryAccessActionMessage, setRepositoryAccessActionMessage] =
    useState<string | null>(null);
  const [pendingRepositoryAccessAction, setPendingRepositoryAccessAction] =
    useState(false);
  const [showRepositoryCredentialComposer, setShowRepositoryCredentialComposer] =
    useState(false);
  const [pendingRepositoryCredentialSave, setPendingRepositoryCredentialSave] =
    useState(false);
  const [repositoryCredentialSaveError, setRepositoryCredentialSaveError] =
    useState<string | null>(null);
  const [pendingPublishCredentialSave, setPendingPublishCredentialSave] =
    useState(false);
  const [publishCredentialSaveError, setPublishCredentialSaveError] =
    useState<string | null>(null);
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
  const [pendingBuildTargetRemovalId, setPendingBuildTargetRemovalId] =
    useState<string | null>(null);
  const nextBuildTargetIdRef = useRef(2);
  const validationTimersRef = useRef<ValidationTimerMap>({});
  const validationTokenRef = useRef<Record<string, number>>({});
  const accessAssessmentTimerRef = useRef<number | undefined>(undefined);
  const accessAssessmentTokenRef = useRef(0);

  const currentStep = WIZARD_STEPS[currentStepIndex];
  const currentStepNumber = currentStepIndex + 1;
  const showPreviousAction = currentStepIndex > 0;
  const showNextAction = currentStep.key !== "review";
  const identityErrors = validateIdentityStep(draft, repositoryInventory);
  const accessErrors = validateAccessStep(draft, repositoryInventory, {
    repositoryAccessAssessment,
    isAssessingRepositoryAccess,
    repositoryAccessError,
    repositoryCredentialId,
    githubAuthProvider,
    isLoadingAuthProviders,
    authProviderError,
    isLoadingRepositoryCredentials,
    repositoryCredentialsError,
    repositoryCredentialCount: repositoryCredentials.length,
  });
  const targetErrors = validateTargetsStep(
    draft,
    pathDiagnostics,
    validatingTargets,
  );
  const buildTargetReferences: ProjectBuildTargetReference[] = draft.buildTargets.map(
    (target) => ({
      id: target.id,
      buildTargetId: null,
      name: target.name.trim() || "Unnamed target",
    }),
  );
  const publishDestinationErrors = validatePublishDestinationDrafts(
    draft.publishDestinations,
    buildTargetReferences,
  );
  const publishDestinationReviewSummary = buildPublishDestinationReviewSummary(
    draft.publishDestinations,
    buildTargetReferences,
  );
  const unboundPublishTargetNames = listUnboundBuildTargetNames(
    draft.publishDestinations,
    buildTargetReferences,
  );
  const pendingBuildTargetRemoval = pendingBuildTargetRemovalId
    ? draft.buildTargets.find((target) => target.id === pendingBuildTargetRemovalId) ??
      null
    : null;
  const pendingBuildTargetBindingImpact = pendingBuildTargetRemoval
    ? collectBuildTargetBindingImpact(
        draft.publishDestinations,
        pendingBuildTargetRemoval.id,
      )
    : [];
  const pathErrors = validatePathStep(draft);
  const repositoryAccessSummary = formatRepositoryAccessSummary(
    draft.repositoryUrl,
    repositoryAccessAssessment,
    isAssessingRepositoryAccess,
    repositoryAccessError,
  );
  const repositoryCredentialOptions = buildRepositoryCredentialOptions(
    repositoryCredentials,
    repositoryCredentialId,
    isLoadingRepositoryCredentials,
  );
  const validatingTargetCount = Object.values(validatingTargets).filter(
    Boolean,
  ).length;
  const currentStepSummary =
    currentStep.key === "identity" ? (
      <MetaRow>
        <MetaItem label="Name">
          {draft.name.trim() || "Pending project name"}
        </MetaItem>
        <MetaItem label="Mode">
          {formatProjectKindLabel(draft.projectKind)}
        </MetaItem>
        <MetaItem label="Engine">{draft.engineKind}</MetaItem>
      </MetaRow>
    ) : currentStep.key === "access" ? (
      <MetaRow>
        <MetaItem label="Remote">
          {draft.repositoryUrl.trim() ? "Configured" : "Pending"}
        </MetaItem>
        <MetaItem label="Poll">
          {`${draft.pollingIntervalSeconds.trim() || "0"}s`}
        </MetaItem>
        <MetaItem label="Access">{repositoryAccessSummary}</MetaItem>
      </MetaRow>
    ) : currentStep.key === "targets" ? (
      <MetaRow>
        <MetaItem label="Targets">
          {formatWizardTargetCount(draft.buildTargets.length)}
        </MetaItem>
        <MetaItem label="Validation">
          {validatingTargetCount > 0
            ? `${validatingTargetCount} running`
            : "Idle"}
        </MetaItem>
      </MetaRow>
    ) : currentStep.key === "publish" ? (
      <MetaRow>
        <MetaItem label="Destinations">
          {`${draft.publishDestinations.length}`}
        </MetaItem>
        <MetaItem label="Bound targets">
          {publishDestinationReviewSummary.reduce(
            (total, destination) =>
              total + destination.bindingTargetNames.length,
            0,
          )}
        </MetaItem>
        <MetaItem label="Validation">
          {hasPublishDestinationValidationErrors(publishDestinationErrors)
            ? "Needs review"
            : "Ready"}
        </MetaItem>
      </MetaRow>
    ) : currentStep.key === "paths" ? (
      <MetaRow>
        <MetaItem label="Artifacts">
          {formatOverrideState(draft.artifactsRootOverride)}
        </MetaItem>
        <MetaItem label="Workspace">
          {formatOverrideState(draft.workspaceRootOverride)}
        </MetaItem>
      </MetaRow>
    ) : (
      <MetaRow>
        <MetaItem label="Project">
          {draft.name.trim() || "Unnamed project"}
        </MetaItem>
        <MetaItem label="Targets">
          {formatWizardTargetCount(draft.buildTargets.length)}
        </MetaItem>
        <MetaItem label="Access">{repositoryAccessSummary}</MetaItem>
      </MetaRow>
    );

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

  const loadAuthProvidersEffect = useEffectEvent(async () => {
    setIsLoadingAuthProviders(true);

    try {
      const providers = await loadAuthProviders();
      const githubProvider =
        providers.find((provider) => provider.provider_id === "github") ?? null;

      startTransition(() => {
        setGithubAuthProvider(githubProvider);
        setAuthProviderError(null);
        setIsLoadingAuthProviders(false);
      });
    } catch (error) {
      startTransition(() => {
        setAuthProviderError(buildProjectErrorMessage(error));
        setIsLoadingAuthProviders(false);
      });
    }
  });

  const listRepositoryCredentialsEffect = useEffectEvent(async () => {
    const settings = await loadSecretSettings();
    return settings.credentials;
  });

  const loadRepositoryCredentialsEffect = useEffectEvent(async () => {
    setIsLoadingRepositoryCredentials(true);

    try {
      const credentials = await listRepositoryCredentialsEffect();

      startTransition(() => {
        setRepositoryCredentials(
          credentials.filter(isRepositoryCredentialSelectable),
        );
        setPublishCredentials(credentials.filter(isItchCredentialSelectable));
        setRepositoryCredentialsError(null);
        setIsLoadingRepositoryCredentials(false);
      });
    } catch (error) {
      startTransition(() => {
        setRepositoryCredentials([]);
        setPublishCredentials([]);
        setRepositoryCredentialsError(buildProjectErrorMessage(error));
        setIsLoadingRepositoryCredentials(false);
      });
    }
  });

  const loadRepositoryAccessAssessmentEffect = useEffectEvent(
    async (
      repositoryUrl: string,
      repositoryVisibility: ProjectDraft["repositoryVisibility"],
      assessmentToken: number,
    ) => {
      try {
        const detection = await detectRepositoryProvider(repositoryUrl);
        if (accessAssessmentTokenRef.current !== assessmentToken) {
          return;
        }

        const assessment = buildRepositoryAccessAssessmentFromDetection(
          detection,
          repositoryVisibility,
        );

        startTransition(() => {
          setRepositoryAccessAssessment(assessment);
          setRepositoryAccessError(null);
          setIsAssessingRepositoryAccess(false);
        });
      } catch (error) {
        if (accessAssessmentTokenRef.current !== assessmentToken) {
          return;
        }

        startTransition(() => {
          setRepositoryAccessAssessment(null);
          setRepositoryAccessError(buildProjectErrorMessage(error));
          setIsAssessingRepositoryAccess(false);
        });
      }
    },
  );

  const handleBindRepositoryAccess = useEffectEvent(async () => {
    if (
      pendingRepositoryAccessAction ||
      !repositoryAccessAssessment ||
      repositoryAccessAssessment.provider_id !== "github"
    ) {
      return;
    }

    startTransition(() => {
      setPendingRepositoryAccessAction(true);
      setRepositoryAccessActionMessage(null);
      setSubmitError(null);
    });

    try {
      let provider = githubAuthProvider;
      if (provider?.status !== "connected" || !provider.credential_id) {
        provider = await loginWithGithubAuth();
      }

      if (!provider.credential_id) {
        throw new Error(
          "GitHub login completed without a reusable credential id.",
        );
      }

      startTransition(() => {
        setGithubAuthProvider(provider);
        setRepositoryCredentialId(provider.credential_id);
        setRepositoryAccessActionMessage(
          "GitHub login connected for this project. Creating the project will save the connection.",
        );
      });
    } catch (error) {
      startTransition(() => {
        setSubmitError(buildProjectErrorMessage(error));
      });
    } finally {
      startTransition(() => {
        setPendingRepositoryAccessAction(false);
      });
    }
  });

  const handleClearRepositoryAccessBinding = useEffectEvent(() => {
    startTransition(() => {
      setRepositoryCredentialId(null);
      setRepositoryAccessActionMessage(
        "Repository credential cleared from the draft.",
      );
    });
  });

  const handleRepositoryCredentialSelectionChange = useEffectEvent(
    (nextCredentialId: string) => {
      startTransition(() => {
        setRepositoryCredentialId(
          nextCredentialId ? Number(nextCredentialId) : null,
        );
        setRepositoryAccessActionMessage(
          nextCredentialId
            ? "Stored repository credential selected for this project. Creating the project will save the connection."
            : "Repository credential cleared from the draft.",
        );
        setRepositoryCredentialSaveError(null);
        setShowRepositoryCredentialComposer(false);
        setSubmitError(null);
      });
    },
  );

  const handleOpenRepositoryCredentialComposer = useEffectEvent(() => {
    startTransition(() => {
      setShowRepositoryCredentialComposer(true);
      setRepositoryCredentialSaveError(null);
      setSubmitError(null);
    });
  });

  const handleCloseRepositoryCredentialComposer = useEffectEvent(() => {
    startTransition(() => {
      setShowRepositoryCredentialComposer(false);
      setRepositoryCredentialSaveError(null);
    });
  });

  const handleSaveRepositoryCredential = useEffectEvent(
    async (input: SaveSecretCredentialInput) => {
      startTransition(() => {
        setPendingRepositoryCredentialSave(true);
        setRepositoryCredentialSaveError(null);
        setSubmitError(null);
      });

      try {
        await saveSecretCredential(input);
        const credentials = await listRepositoryCredentialsEffect();
        const createdCredential = credentials.find(
          (credential) => credential.name === input.name.trim(),
        );
        if (!createdCredential) {
          throw new Error(
            "The saved repository credential could not be reloaded.",
          );
        }

        startTransition(() => {
          setRepositoryCredentials(
            credentials.filter(isRepositoryCredentialSelectable),
          );
          setPublishCredentials(credentials.filter(isItchCredentialSelectable));
          setRepositoryCredentialsError(null);
          setIsLoadingRepositoryCredentials(false);
          setRepositoryCredentialId(createdCredential.credential_id);
          setRepositoryAccessActionMessage(
            "Repository credential created and selected for this project.",
          );
          setShowRepositoryCredentialComposer(false);
        });
      } catch (error) {
        startTransition(() => {
          setRepositoryCredentialSaveError(buildProjectErrorMessage(error));
        });
      } finally {
        startTransition(() => {
          setPendingRepositoryCredentialSave(false);
        });
      }
    },
  );

  const handleSavePublishCredential = useEffectEvent(
    async (destinationId: string, input: SaveSecretCredentialInput) => {
      startTransition(() => {
        setPendingPublishCredentialSave(true);
        setPublishCredentialSaveError(null);
        setSubmitError(null);
      });

      try {
        await saveSecretCredential(input);
        const credentials = await listRepositoryCredentialsEffect();
        const createdCredential = credentials.find(
          (credential) =>
            credential.name === input.name.trim() &&
            credential.kind === input.kind,
        );

        if (!createdCredential) {
          throw new Error(
            "The saved publish credential could not be reloaded.",
          );
        }

        startTransition(() => {
          setRepositoryCredentials(
            credentials.filter(isRepositoryCredentialSelectable),
          );
          setPublishCredentials(credentials.filter(isItchCredentialSelectable));
          setDraft((current) => ({
            ...current,
            publishDestinations: current.publishDestinations.map(
              (destination) =>
                destination.id === destinationId
                  ? {
                      ...destination,
                      credentialsId: createdCredential.credential_id,
                    }
                  : destination,
            ),
          }));
          setPublishCredentialSaveError(null);
        });
      } catch (error) {
        startTransition(() => {
          setPublishCredentialSaveError(buildProjectErrorMessage(error));
        });
      } finally {
        startTransition(() => {
          setPendingPublishCredentialSave(false);
        });
      }
    },
  );

  useEffect(() => {
    void loadRepositoryInventoryEffect();
    void loadAuthProvidersEffect();
    void loadRepositoryCredentialsEffect();

    return () => {
      if (accessAssessmentTimerRef.current !== undefined) {
        window.clearTimeout(accessAssessmentTimerRef.current);
      }
      for (const timerId of Object.values(validationTimersRef.current)) {
        if (timerId !== undefined) {
          window.clearTimeout(timerId);
        }
      }
    };
  }, []);

  useEffect(() => {
    const normalizedUrl = draft.repositoryUrl.trim();
    const repositoryVisibility = draft.repositoryVisibility;
    accessAssessmentTokenRef.current += 1;
    const assessmentToken = accessAssessmentTokenRef.current;

    if (accessAssessmentTimerRef.current !== undefined) {
      window.clearTimeout(accessAssessmentTimerRef.current);
      accessAssessmentTimerRef.current = undefined;
    }

    if (
      !normalizedUrl ||
      !(
        normalizedUrl.startsWith("https://") ||
        normalizedUrl.startsWith("http://")
      )
    ) {
      startTransition(() => {
        setRepositoryAccessAssessment(null);
        setRepositoryAccessError(null);
        setIsAssessingRepositoryAccess(false);
        setRepositoryAccessActionMessage(null);
      });
      return;
    }

    startTransition(() => {
      setIsAssessingRepositoryAccess(true);
      setRepositoryAccessError(null);
      setRepositoryAccessActionMessage(null);
    });

    accessAssessmentTimerRef.current = window.setTimeout(() => {
      void loadRepositoryAccessAssessmentEffect(
        normalizedUrl,
        repositoryVisibility,
        assessmentToken,
      );
    }, 250);

    return () => {
      if (accessAssessmentTimerRef.current !== undefined) {
        window.clearTimeout(accessAssessmentTimerRef.current);
        accessAssessmentTimerRef.current = undefined;
      }
    };
  }, [draft.repositoryUrl, draft.repositoryVisibility]);

  useEffect(() => {
    if (supportsShellRepositoryLoginAction(repositoryAccessAssessment)) {
      return;
    }

    startTransition(() => {
      setRepositoryCredentialId(null);
      setShowRepositoryCredentialComposer(false);
      setRepositoryCredentialSaveError(null);
    });
  }, [repositoryAccessAssessment?.auth_requirement]);

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

  const finalizeBuildTargetRemoval = useEffectEvent((targetId: string) => {
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
        publishDestinations: removeBuildTargetBindings(
          current.publishDestinations,
          targetId,
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

  const handleRemoveBuildTarget = useEffectEvent((targetId: string) => {
    if (
      collectBuildTargetBindingImpact(draft.publishDestinations, targetId).length >
      0
    ) {
      startTransition(() => {
        setPendingBuildTargetRemovalId(targetId);
      });
      return;
    }

    finalizeBuildTargetRemoval(targetId);
  });

  const handleConfirmBuildTargetRemoval = useEffectEvent(() => {
    if (!pendingBuildTargetRemovalId) {
      return;
    }

    finalizeBuildTargetRemoval(pendingBuildTargetRemovalId);
    startTransition(() => {
      setPendingBuildTargetRemovalId(null);
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

    if (currentStep.key === "publish") {
      if (hasPublishDestinationValidationErrors(publishDestinationErrors)) {
        startTransition(() => {
          setAttemptedSteps((current) => ({
            ...current,
            publish: true,
          }));
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
      publishErrors: publishDestinationErrors,
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
        buildCreateProjectInput(
          draft,
          repositoryAccessAssessment,
          resolveRepositoryCredentialIdForSave(
            repositoryAccessAssessment,
            repositoryCredentialId,
          ),
        ),
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
    <FocusPageFrame
      bodyClassName="wizard-shell__body"
      className="wizard-shell"
      description="Register a repository project, define how HGP should poll it, and declare the Unity targets the runtime will execute."
      eyebrow="Create Project"
      summary={
        <MetaRow>
          <MetaItem label="Step">
            {`${currentStepNumber} of ${WIZARD_STEPS.length}`}
          </MetaItem>
          <MetaItem label="Mode">
            {formatProjectKindLabel(draft.projectKind)}
          </MetaItem>
          <MetaItem label="Engine">{draft.engineKind}</MetaItem>
          <MetaItem label="Targets">
            {formatWizardTargetCount(draft.buildTargets.length)}
          </MetaItem>
        </MetaRow>
      }
      title="Create Project"
    >
      {inventoryError ? (
        <p className="feed-banner feed-banner--error">{inventoryError}</p>
      ) : null}

      {submitError ? (
        <p className="feed-banner feed-banner--error">{submitError}</p>
      ) : null}

      <div className="wizard-stage-shell">
        <SurfacePanel
          className="wizard-progress-panel"
          description="Move across completed steps without losing the current project draft."
          eyebrow="Progress"
          headerSeparated
          summary={
            <MetaRow>
              <MetaItem label="Current">{currentStep.label}</MetaItem>
              <MetaItem label="Draft">
                {draft.name.trim() || "Unnamed project"}
              </MetaItem>
            </MetaRow>
          }
          title="Wizard Steps"
          tone="inset"
        >
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
        </SurfacePanel>

        <SurfacePanel
          className="wizard-stage-panel"
          description={currentStep.description}
          eyebrow={`Step ${currentStepNumber} of ${WIZARD_STEPS.length}`}
          headerSeparated
          summary={currentStepSummary}
          title={currentStep.label}
          tone="section"
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

            <RepositoryEngineField
              error={
                shouldShowFieldError(
                  attemptedSteps.identity,
                  touchedFields,
                  "engineKind",
                )
                  ? identityErrors.engineKind
                  : undefined
              }
              onBlur={() => markFieldTouched("engineKind")}
              onChange={(event) => {
                const engineKind = event.currentTarget
                  .value as ProjectDraft["engineKind"];
                startTransition(() => {
                  setDraft((current) => ({
                    ...current,
                    engineKind,
                  }));
                });
                markFieldTouched("engineKind");
              }}
              value={draft.engineKind}
            />

            <div className="wizard-callout wizard-callout--compact wizard-callout--support">
              <p className="wizard-callout__copy">
                Repository projects let the runtime poll a remote Git
                repository on a fixed cadence and queue automation when a new
                release tag appears. Only Unity is currently supported; the
                future engines stay visible so the model does not keep lying
                about the roadmap.
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
              hint="Use the HTTPS remote that HGP will poll and clone."
              label="Repository URL"
              leadingIcon="server"
              onBlur={() => {
                markFieldTouched("repositoryUrl");
                markFieldTouched("repositoryAccess");
              }}
              onChange={(event) => {
                const nextValue = event.currentTarget.value;
                startTransition(() => {
                  setDraft((current) => ({
                    ...current,
                    repositoryUrl: nextValue,
                  }));
                });
                markFieldTouched("repositoryUrl");
                markFieldTouched("repositoryAccess");
              }}
              placeholder="https://github.com/org/project.git"
              value={draft.repositoryUrl}
            />
            <SelectField
              hint="Tell HGP whether this remote should be treated as public or private."
              label="Repository visibility"
              onBlur={() => markFieldTouched("repositoryAccess")}
              onChange={(event) => {
                const nextValue = event.currentTarget
                  .value as ProjectDraft["repositoryVisibility"];
                startTransition(() => {
                  setDraft((current) => ({
                    ...current,
                    repositoryVisibility: nextValue,
                  }));
                });
                markFieldTouched("repositoryAccess");
              }}
              options={REPOSITORY_VISIBILITY_OPTIONS}
              value={draft.repositoryVisibility}
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
              hint="Optional. HGP uses this when branch-aware operations need a default ref."
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

            <div className="wizard-callout wizard-callout--compact wizard-callout--auth wizard-callout--support">
              <div className="wizard-callout__header">
                <div>
                  <p className="wizard-callout__title">Repository access</p>
                  <p className="wizard-callout__copy">
                    {resolveRepositoryAccessCopy(
                      draft.repositoryUrl,
                      repositoryAccessAssessment,
                      isAssessingRepositoryAccess,
                      repositoryAccessError,
                    )}
                  </p>
                </div>

                <div className="wizard-callout__badges">
                  <Badge
                    tone={resolveRepositoryAccessBadgeTone(
                      repositoryAccessAssessment,
                      isAssessingRepositoryAccess,
                      repositoryAccessError,
                    )}
                  >
                    {formatRepositoryAccessStatus(
                      draft.repositoryUrl,
                      repositoryAccessAssessment,
                      isAssessingRepositoryAccess,
                      repositoryAccessError,
                    )}
                  </Badge>
                </div>
              </div>

              {draft.repositoryUrl.trim() ||
              isAssessingRepositoryAccess ||
              repositoryAccessAssessment ||
              repositoryAccessError ? (
                <MetaRow className="wizard-callout__meta">
                  <MetaItem label="Provider">
                    {formatRepositoryAccessProviderLabel(
                      repositoryAccessAssessment,
                      isAssessingRepositoryAccess,
                      repositoryAccessError,
                    )}
                  </MetaItem>
                  <MetaItem label="Visibility">
                    {formatRepositoryVisibilityLabel(
                      repositoryAccessAssessment,
                      isAssessingRepositoryAccess,
                      repositoryAccessError,
                    )}
                  </MetaItem>
                  <MetaItem label="Login">
                    {formatRepositoryLoginStatus(
                      repositoryAccessAssessment,
                      githubAuthProvider,
                      isLoadingAuthProviders,
                    )}
                  </MetaItem>
                  <MetaItem label="Connection">
                    {formatRepositoryBindingStatus(
                      repositoryAccessAssessment,
                      repositoryCredentialId,
                      pendingRepositoryAccessAction,
                    )}
                  </MetaItem>
                </MetaRow>
              ) : null}

              {repositoryAccessActionMessage ? (
                <p className="feed-banner feed-banner--info">
                  {repositoryAccessActionMessage}
                </p>
              ) : null}

              {shouldShowFieldError(
                attemptedSteps.access,
                touchedFields,
                "repositoryAccess",
              ) && accessErrors.repositoryAccess ? (
                <p className="ui-field__error">{accessErrors.repositoryAccess}</p>
              ) : null}

              {shouldShowRepositoryLoginAction(repositoryAccessAssessment) ? (
                <>
                  <SelectField
                    disabled={
                      isLoadingRepositoryCredentials ||
                      pendingRepositoryAccessAction ||
                      pendingRepositoryCredentialSave
                    }
                    hint={formatRepositoryCredentialFieldHint(
                      repositoryAccessAssessment,
                      isLoadingRepositoryCredentials,
                    )}
                    label="Repository credential"
                    onChange={(event) =>
                      handleRepositoryCredentialSelectionChange(
                        event.currentTarget.value,
                      )
                    }
                    options={repositoryCredentialOptions}
                    value={repositoryCredentialId?.toString() ?? ""}
                  />

                  <div className="wizard-callout__actions">
                    {supportsShellRepositoryLoginAction(
                      repositoryAccessAssessment,
                    ) ? (
                      <Button
                        leadingIcon="key"
                        onClick={handleBindRepositoryAccess}
                        disabled={
                          pendingRepositoryAccessAction ||
                          pendingRepositoryCredentialSave
                        }
                        size="sm"
                        variant={
                          repositoryCredentialId !== null
                            ? "secondary"
                            : "primary"
                        }
                      >
                        {pendingRepositoryAccessAction
                          ? "Connecting login..."
                          : formatRepositoryBindingActionLabel(
                              repositoryAccessAssessment,
                              githubAuthProvider,
                              repositoryCredentialId,
                            )}
                      </Button>
                    ) : null}

                    {repositoryCredentialId !== null ? (
                      <Button
                        onClick={handleClearRepositoryAccessBinding}
                        size="sm"
                        variant="ghost"
                      >
                        Disconnect
                      </Button>
                    ) : null}

                    {!showRepositoryCredentialComposer ? (
                      <Button
                        disabled={pendingRepositoryCredentialSave}
                        leadingIcon="plus"
                        onClick={handleOpenRepositoryCredentialComposer}
                        size="sm"
                        variant="ghost"
                      >
                        New credential
                      </Button>
                    ) : null}

                    {onManageAuth &&
                    supportsShellRepositoryLoginAction(
                      repositoryAccessAssessment,
                    ) ? (
                      <Button
                        onClick={onManageAuth}
                        size="sm"
                        variant="ghost"
                      >
                        Open accounts
                      </Button>
                    ) : null}
                  </div>

                  {showRepositoryCredentialComposer &&
                  repositoryAccessAssessment ? (
                    <RepositoryCredentialComposer
                      isSaving={pendingRepositoryCredentialSave}
                      onCancel={handleCloseRepositoryCredentialComposer}
                      onSave={handleSaveRepositoryCredential}
                      providerLabel={repositoryAccessAssessment.provider_label}
                      saveError={repositoryCredentialSaveError}
                    />
                  ) : null}
                </>
              ) : null}
            </div>
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

            {pendingBuildTargetRemoval ? (
              <div className="wizard-callout wizard-callout--compact wizard-callout--auth">
                <div className="wizard-callout__header">
                  <div>
                    <p className="wizard-callout__title">
                      Confirm build target removal
                    </p>
                    <p className="wizard-callout__copy">
                      Removing
                      {" "}
                      {pendingBuildTargetRemoval.name.trim() ||
                        "this build target"}
                      {" "}
                      also removes publish bindings from
                      {" "}
                      {pendingBuildTargetBindingImpact.join(", ")}.
                    </p>
                  </div>
                </div>

                <div className="wizard-callout__actions">
                  <Button
                    leadingIcon="trash"
                    onClick={handleConfirmBuildTargetRemoval}
                    size="sm"
                    variant="primary"
                  >
                    Remove target and bindings
                  </Button>
                  <Button
                    onClick={() => setPendingBuildTargetRemovalId(null)}
                    size="sm"
                    variant="ghost"
                  >
                    Cancel
                  </Button>
                </div>
              </div>
            ) : null}

            {draft.buildTargets.map((target, index) => {
              const diagnostics = pathDiagnostics[target.id];
              const fieldErrors = targetErrors.targets[target.id] ?? {};

              return (
                <VerticalAccordion
                  bodyClassName="wizard-target-card__body"
                  bodyInset
                  className="wizard-target-card"
                  collapsedToggleLabel={`Expand build target ${index + 1}`}
                  expandedToggleLabel={`Collapse build target ${index + 1}`}
                  header={
                    <div className="wizard-target-card__header">
                      <div className="wizard-target-card__top-row">
                        <p className="wizard-target-card__eyebrow">
                          Build target {index + 1}
                        </p>
                        <IconButton
                          className="wizard-target-card__remove"
                          disabled={draft.buildTargets.length === 1}
                          icon="trash"
                          label={`Remove build target ${index + 1}`}
                          onClick={() => handleRemoveBuildTarget(target.id)}
                          size="sm"
                          variant="ghost"
                        />
                      </div>

                      <div className="wizard-target-card__title-block">
                        <h3 className="wizard-target-card__title">
                          {target.name.trim() || "Unnamed target"}
                        </h3>
                      </div>

                      <div className="wizard-target-card__badges">
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
                      </div>
                    </div>
                  }
                  headerSeparated
                  key={target.id}
                  onOpenChange={(nextOpen) =>
                    handleTargetAccordionChange(target.id, nextOpen)
                  }
                  open={Boolean(expandedTargetIds[target.id])}
                  tone="section"
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
                          buildTargetFieldKey(target.id, "targetPlatform"),
                        )
                          ? fieldErrors.targetPlatform
                          : undefined
                      }
                      hint="This writes the Unity targetPlatform contract field directly."
                      label="Unity target platform"
                      onBlur={() =>
                        markFieldTouched(
                          buildTargetFieldKey(target.id, "targetPlatform"),
                        )
                      }
                      onChange={(event) => {
                        updateBuildTarget(target.id, {
                          targetPlatform: normalizeUnityTargetPlatformValue(
                            event.currentTarget.value,
                          ),
                        });
                        markFieldTouched(
                          buildTargetFieldKey(target.id, "targetPlatform"),
                        );
                      }}
                      options={PLATFORM_OPTIONS}
                      value={normalizeUnityTargetPlatformValue(
                        target.targetPlatform,
                      )}
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
                      label="Unity build method"
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

        {currentStep.key === "publish" ? (
          <PublishDestinationsEditor
            buildTargets={buildTargetReferences}
            credentialSaveError={publishCredentialSaveError}
            credentials={publishCredentials}
            destinations={draft.publishDestinations}
            disabled={isSubmitting}
            errors={
              attemptedSteps.publish ? publishDestinationErrors : undefined
            }
            isSavingCredential={pendingPublishCredentialSave}
            onChange={(nextPublishDestinations) => {
              startTransition(() => {
                setDraft((current) => ({
                  ...current,
                  publishDestinations: nextPublishDestinations,
                }));
              });
            }}
            onSaveCredential={handleSavePublishCredential}
          />
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
            <SurfacePanel
              className="wizard-review-panel"
              description={draft.repositoryUrl.trim() || "Repository URL not set yet."}
              eyebrow="Project"
              headerSeparated
              summary={
                <MetaRow>
                  <MetaItem label="Engine">{draft.engineKind}</MetaItem>
                  <MetaItem label="Poll">
                    {`${draft.pollingIntervalSeconds.trim() || "0"}s`}
                  </MetaItem>
                  <MetaItem label="Access">{repositoryAccessSummary}</MetaItem>
                </MetaRow>
              }
              title={draft.name.trim() || "Unnamed project"}
              tone="inset"
            >
              <p className="wizard-summary-panel__copy">
                {formatProjectKindLabel(draft.projectKind)} with
                {` ${formatWizardTargetCount(draft.buildTargets.length)} configured for registration.`}
              </p>
            </SurfacePanel>

            <SurfacePanel
              className="wizard-review-panel"
              description="Host-native Unity targets that HGP will execute for this repository."
              eyebrow="Build Targets"
              headerSeparated
              title="Target Review"
              tone="inset"
            >
              <div className="wizard-summary-list">
                {draft.buildTargets.map((target) => (
                  <div className="wizard-summary-list__item" key={target.id}>
                    <div className="wizard-summary-list__title-row">
                      <strong>
                        {target.name.trim() || "Unnamed target"}
                      </strong>
                      <Badge tone="neutral">
                        {target.targetPlatform || "Unity target pending"}
                      </Badge>
                    </div>
                    <p className="wizard-summary-list__copy">
                      {target.buildMethod.trim() || "Unity build method pending"}
                    </p>
                    <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
                      {target.unityExecutablePath.trim() ||
                        "Unity executable pending"}
                    </p>
                  </div>
                ))}
              </div>
            </SurfacePanel>

            <SurfacePanel
              className="wizard-review-panel"
              description="Destination-specific publish bindings and credential readiness."
              eyebrow="Publish Destinations"
              headerSeparated
              title="Destination Review"
              tone="inset"
            >
              <div className="wizard-summary-list">
                {publishDestinationReviewSummary.length === 0 ? (
                  <div className="wizard-summary-list__item">
                    <div className="wizard-summary-list__title-row">
                      <strong>No publish destinations configured</strong>
                      <Badge tone="muted">valid</Badge>
                    </div>
                    <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
                      Every build target will keep its artifact under the
                      runtime-managed output root.
                    </p>
                  </div>
                ) : (
                  publishDestinationReviewSummary.map((destination) => (
                    <div className="wizard-summary-list__item" key={destination.id}>
                      <div className="wizard-summary-list__title-row">
                        <strong>{destination.name}</strong>
                        <Badge tone={destination.enabled ? "strong" : "muted"}>
                          {destination.kindLabel}
                        </Badge>
                      </div>
                      <p className="wizard-summary-list__copy">
                        {destination.bindingTargetNames.length > 0
                          ? destination.bindingTargetNames.join(", ")
                          : "No build targets bound yet."}
                      </p>
                      <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
                        {destination.missingCredential
                          ? "Credential still missing."
                          : destination.usesHostTransportProbe
                            ? "Uses host butler resolution from PATH."
                            : "Publish transport and credentials are bound from the draft."}
                      </p>
                    </div>
                  ))
                )}

                <div className="wizard-summary-list__item">
                  <div className="wizard-summary-list__title-row">
                    <strong>Unbound build targets</strong>
                    <Badge tone="muted">
                      {unboundPublishTargetNames.length === 0 ? "none" : "kept local"}
                    </Badge>
                  </div>
                  <p className="wizard-summary-list__copy wizard-summary-list__copy--muted">
                    {unboundPublishTargetNames.length > 0
                      ? unboundPublishTargetNames.join(", ")
                      : "Every configured build target is bound to at least one publish destination."}
                  </p>
                </div>
              </div>
            </SurfacePanel>

            <SurfacePanel
              className="wizard-review-panel"
              description="Repository-specific overrides for artifacts and managed workspaces."
              eyebrow="Paths"
              headerSeparated
              title="Path Review"
              tone="inset"
            >
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
            </SurfacePanel>
          </div>
        ) : null}
        </SurfacePanel>
      </div>

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
    </FocusPageFrame>
  );
}

function formatProjectKindLabel(projectKind: ProjectDraft["projectKind"]) {
  return projectKind === "repository"
    ? "Repository project"
    : "Local workspace project";
}

function formatOverrideState(value: string) {
  return value.trim() ? "Override" : "Default path";
}

function formatWizardTargetCount(targetCount: number) {
  return `${targetCount} target${targetCount === 1 ? "" : "s"}`;
}

function createEmptyBuildTargetDraft(index: number): BuildTargetDraft {
  return {
    id: `target-${index}`,
    name: "",
    targetPlatform: "",
    buildMethod: "",
    unityExecutablePath: "",
  };
}

function validateIdentityStep(
  draft: ProjectDraft,
  repositoryInventory: RepositoryInspectionEntry[],
) {
  const errors: { name?: string; projectKind?: string; engineKind?: string } =
    {};
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

  if (draft.engineKind !== "unity") {
    errors.engineKind =
      "Only Unity is currently supported even though future engines are listed.";
  }

  return errors;
}

function validateAccessStep(
  draft: ProjectDraft,
  repositoryInventory: RepositoryInspectionEntry[],
  authState: {
    repositoryAccessAssessment: RepositoryAccessAssessment | null;
    isAssessingRepositoryAccess: boolean;
    repositoryAccessError: string | null;
    repositoryCredentialId: number | null;
    githubAuthProvider: AuthProviderStatus | null;
    isLoadingAuthProviders: boolean;
    authProviderError: string | null;
    isLoadingRepositoryCredentials: boolean;
    repositoryCredentialsError: string | null;
    repositoryCredentialCount: number;
  },
) {
  const errors: {
    repositoryUrl?: string;
    defaultBranch?: string;
    pollingIntervalSeconds?: string;
    repositoryAccess?: string;
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
    errors.repositoryUrl = "This remote is already registered in HGP.";
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

  if (
    !errors.repositoryUrl &&
    (normalizedUrl.startsWith("https://") || normalizedUrl.startsWith("http://"))
  ) {
    if (authState.isAssessingRepositoryAccess) {
      errors.repositoryAccess = "Repository access is still being checked.";
    } else if (authState.repositoryAccessError) {
      errors.repositoryAccess =
        "Repository access could not be checked from the desktop shell.";
    } else if (!authState.repositoryAccessAssessment) {
      errors.repositoryAccess = "Repository access has not been checked yet.";
    } else if (
      authState.repositoryAccessAssessment.visibility === "invalid" ||
      authState.repositoryAccessAssessment.visibility === "unknown"
    ) {
      errors.repositoryAccess = authState.repositoryAccessAssessment.message;
    } else if (
      authState.repositoryAccessAssessment.auth_requirement === "required"
    ) {
      if (
        !supportsShellRepositoryLoginAction(
          authState.repositoryAccessAssessment,
        )
      ) {
        const providerLabel =
          authState.repositoryAccessAssessment.provider_id === "unknown"
            ? "this host"
            : authState.repositoryAccessAssessment.provider_label || "this host";
        errors.repositoryAccess =
          authState.repositoryAccessAssessment.provider_id === "unknown"
            ? "Private repositories are not supported for this host yet. Only public repositories can be added right now."
            : `Private ${providerLabel} repositories are not supported yet. Only public repositories are available for this platform right now.`;
      } else if (authState.isLoadingRepositoryCredentials) {
        errors.repositoryAccess =
          "Repository credentials are still loading from the desktop shell.";
      } else if (authState.repositoryCredentialsError) {
        errors.repositoryAccess =
          "Repository credentials could not be loaded from the desktop shell.";
      } else if (!authState.repositoryCredentialId) {
        const providerLabel =
          authState.repositoryAccessAssessment.provider_label || "Repository";

        if (
          authState.repositoryAccessAssessment.provider_id === "github" &&
          authState.repositoryAccessAssessment.supports_interactive_login
        ) {
        if (authState.isLoadingAuthProviders) {
          errors.repositoryAccess = "GitHub login status is still loading.";
        } else if (authState.authProviderError) {
          errors.repositoryAccess =
            "GitHub login status could not be loaded from the desktop shell.";
        } else if (authState.githubAuthProvider?.status !== "connected") {
          errors.repositoryAccess =
            "Private GitHub repository detected. Log in and connect a GitHub credential, or select another stored repository credential, before setup can continue.";
        } else {
          errors.repositoryAccess =
            "Private GitHub repository detected. Connect a credential to this project before setup can continue.";
        }
        } else if (authState.repositoryCredentialCount === 0) {
          errors.repositoryAccess =
            `Private ${providerLabel} repository detected. No stored repository credentials are available for this project yet.`;
        } else {
          errors.repositoryAccess =
            `Private ${providerLabel} repository detected. Select a stored repository credential before setup can continue.`;
        }
      } else {
        errors.repositoryAccess = undefined;
      }
    }
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
  return Boolean(errors.name || errors.projectKind || errors.engineKind);
}

function hasAccessErrors(errors: ReturnType<typeof validateAccessStep>) {
  return Boolean(
    errors.repositoryUrl ||
    errors.defaultBranch ||
    errors.pollingIntervalSeconds ||
    errors.repositoryAccess,
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
    errors.targetPlatform ||
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
  publishErrors: ReturnType<typeof validatePublishDestinationDrafts>;
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
  if (hasPublishDestinationValidationErrors(input.publishErrors)) {
    return "publish";
  }
  if (hasPathErrors(input.pathErrors)) {
    return "paths";
  }

  return null;
}

function buildCreateProjectInput(
  draft: ProjectDraft,
  repositoryAccessAssessment: RepositoryAccessAssessment | null,
  repositoryCredentialId: number | null,
): CreateRepositoryProjectInput {
  return {
    name: draft.name.trim(),
    engine_kind: draft.engineKind,
    repository_url: draft.repositoryUrl.trim(),
    repository_access_assessment: repositoryAccessAssessment,
    repository_credentials_id: repositoryCredentialId,
    default_branch: optionalTrimmedString(draft.defaultBranch),
    artifacts_root_override: optionalTrimmedString(draft.artifactsRootOverride),
    workspace_root_override: optionalTrimmedString(draft.workspaceRootOverride),
    polling_interval_seconds: Number(draft.pollingIntervalSeconds.trim()),
    build_targets: draft.buildTargets.map((target) => ({
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
    publish_targets: buildCreateProjectPublishTargetsInput(
      draft.publishDestinations,
      draft.buildTargets.map((target) => ({
        id: target.id,
        buildTargetId: null,
        name: target.name.trim() || "Unnamed target",
      })),
    ),
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

function formatGithubAuthProviderStatus(
  provider: AuthProviderStatus | null,
  isLoadingAuthProviders: boolean,
) {
  if (isLoadingAuthProviders) {
    return "loading";
  }

  if (!provider) {
    return "unavailable";
  }

  if (provider.status === "connected") {
    return "connected";
  }

  if (provider.status === "disconnected") {
    return "ready to connect";
  }

  return "unavailable";
}
function resolveRepositoryAccessBadgeTone(
  assessment: RepositoryAccessAssessment | null,
  isAssessingRepositoryAccess: boolean,
  repositoryAccessError: string | null,
): "strong" | "neutral" | "muted" {
  if (isAssessingRepositoryAccess) {
    return "muted";
  }

  if (repositoryAccessError) {
    return "neutral";
  }

  if (!assessment) {
    return "muted";
  }

  if (assessment.visibility === "public") {
    return "strong";
  }

  if (assessment.visibility === "private") {
    return assessment.supports_interactive_login ? "neutral" : "muted";
  }

  if (assessment.visibility === "invalid") {
    return "neutral";
  }

  return "muted";
}

function formatRepositoryAccessStatus(
  repositoryUrl: string,
  assessment: RepositoryAccessAssessment | null,
  isAssessingRepositoryAccess: boolean,
  repositoryAccessError: string | null,
) {
  if (!repositoryUrl.trim()) {
    return "pending";
  }

  if (isAssessingRepositoryAccess) {
    return "checking";
  }

  if (repositoryAccessError) {
    return "check failed";
  }

  if (!assessment) {
    return "pending";
  }

  switch (assessment.visibility) {
    case "public":
      return "public";
    case "private":
      return assessment.supports_interactive_login
        ? "login required"
        : "unsupported";
    case "invalid":
      return "invalid";
    default:
      return "unknown";
  }
}

function resolveRepositoryAccessCopy(
  repositoryUrl: string,
  assessment: RepositoryAccessAssessment | null,
  isAssessingRepositoryAccess: boolean,
  repositoryAccessError: string | null,
) {
  if (!repositoryUrl.trim()) {
    return "Paste a repository URL, choose whether the repository is public or private, and HGP will detect which platform owns the host.";
  }

  if (isAssessingRepositoryAccess) {
    return "HGP is identifying which platform owns this repository URL and whether private login is supported for the selected visibility.";
  }

  if (repositoryAccessError) {
    return repositoryAccessError;
  }

  if (assessment) {
    return assessment.message;
  }

  return "Repository access has not been checked yet.";
}

function formatRepositoryAccessSummary(
  repositoryUrl: string,
  assessment: RepositoryAccessAssessment | null,
  isAssessingRepositoryAccess: boolean,
  repositoryAccessError: string | null,
) {
  if (!repositoryUrl.trim()) {
    return "Pending";
  }

  if (isAssessingRepositoryAccess) {
    return "Checking";
  }

  if (repositoryAccessError) {
    return "Check failed";
  }

  if (!assessment) {
    return "Pending";
  }

  switch (assessment.visibility) {
    case "public":
      return "Public";
    case "private":
      return "Private";
    case "invalid":
      return "Invalid";
    default:
      return "Unknown";
  }
}

function formatRepositoryAccessProviderLabel(
  assessment: RepositoryAccessAssessment | null,
  isAssessingRepositoryAccess: boolean,
  repositoryAccessError: string | null,
) {
  if (isAssessingRepositoryAccess) {
    return "Detecting";
  }

  if (repositoryAccessError || !assessment) {
    return "Pending";
  }

  return assessment.provider_label;
}

function formatRepositoryVisibilityLabel(
  assessment: RepositoryAccessAssessment | null,
  isAssessingRepositoryAccess: boolean,
  repositoryAccessError: string | null,
) {
  if (isAssessingRepositoryAccess) {
    return "Checking";
  }

  if (repositoryAccessError) {
    return "Needs review";
  }

  if (!assessment) {
    return "Pending";
  }

  switch (assessment.visibility) {
    case "public":
      return "Public";
    case "private":
      return "Private";
    case "invalid":
      return "Invalid";
    default:
      return "Unknown";
  }
}

function formatRepositoryLoginStatus(
  assessment: RepositoryAccessAssessment | null,
  githubAuthProvider: AuthProviderStatus | null,
  isLoadingAuthProviders: boolean,
) {
  if (!assessment) {
    return "Pending";
  }

  if (assessment.auth_requirement === "none") {
    return "Not required";
  }

  if (!assessment.supports_interactive_login) {
    return "Not available";
  }

  if (assessment.provider_id === "github") {
    return formatGithubAuthProviderStatus(
      githubAuthProvider,
      isLoadingAuthProviders,
    );
  }

  return "Required";
}

function formatRepositoryBindingStatus(
  assessment: RepositoryAccessAssessment | null,
  repositoryCredentialId: number | null,
  pendingRepositoryAccessAction: boolean,
) {
  if (!assessment) {
    return "Pending";
  }

  if (pendingRepositoryAccessAction) {
    return "Connecting";
  }

  if (assessment.auth_requirement === "none") {
    return "Not required";
  }

  if (!supportsShellRepositoryLoginAction(assessment)) {
    return "Unavailable";
  }

  return repositoryCredentialId ? "Selected" : "Pending";
}

function formatRepositoryBindingActionLabel(
  assessment: RepositoryAccessAssessment | null,
  githubAuthProvider: AuthProviderStatus | null,
  repositoryCredentialId: number | null,
) {
  if (!assessment) {
    return "Connect credential";
  }

  if (repositoryCredentialId) {
    return assessment.provider_id === "github"
      ? "Reconnect GitHub login"
      : "Change credential";
  }

  if (assessment.provider_id === "github") {
    return githubAuthProvider?.status === "connected"
      ? "Connect GitHub login"
      : "Log in and connect";
  }

  return "Select credential";
}

function shouldShowRepositoryLoginAction(
  assessment: RepositoryAccessAssessment | null,
) {
  return supportsShellRepositoryLoginAction(assessment);
}

function supportsShellRepositoryLoginAction(
  assessment: RepositoryAccessAssessment | null,
) {
  return Boolean(
    assessment?.auth_requirement === "required" &&
      assessment.supports_interactive_login &&
      assessment.provider_id === "github",
  );
}

function resolveRepositoryCredentialIdForSave(
  assessment: RepositoryAccessAssessment | null,
  repositoryCredentialId: number | null,
) {
  if (!assessment) {
    return repositoryCredentialId;
  }

  if (assessment.auth_requirement !== "required") {
    return null;
  }

  return repositoryCredentialId;
}

function formatRepositoryCredentialFieldHint(
  assessment: RepositoryAccessAssessment | null,
  isLoadingRepositoryCredentials: boolean,
) {
  if (isLoadingRepositoryCredentials) {
    return "Loading stored repository credentials...";
  }

  if (!assessment || assessment.auth_requirement !== "required") {
    return "Public repositories can keep this empty.";
  }

  return "Choose a stored GitHub credential or use the login action below.";
}

function buildRepositoryAccessAssessmentFromDetection(
  detection: RepositoryProviderDetection,
  repositoryVisibility: ProjectDraft["repositoryVisibility"],
): RepositoryAccessAssessment {
  if (repositoryVisibility === "public") {
    return {
      provider_id: detection.provider_id,
      provider_label: detection.provider_label,
      instance_url: detection.instance_url,
      normalized_url: detection.normalized_url,
      visibility: "public",
      auth_requirement: "none",
      auth_status: "not_required",
      supports_interactive_login: detection.supports_interactive_login,
      message:
        "Public repository selected. HGP will poll and clone this remote without repository authentication.",
    };
  }

  if (detection.supports_interactive_login && detection.provider_id === "github") {
    return {
      provider_id: detection.provider_id,
      provider_label: detection.provider_label,
      instance_url: detection.instance_url,
      normalized_url: detection.normalized_url,
      visibility: "private",
      auth_requirement: "required",
      auth_status: "required_unbound",
      supports_interactive_login: detection.supports_interactive_login,
      message:
        "Private GitHub repository selected. Log in and connect this project before setup can continue.",
    };
  }

  if (detection.provider_id === "unknown") {
    return {
      provider_id: detection.provider_id,
      provider_label: detection.provider_label,
      instance_url: detection.instance_url,
      normalized_url: detection.normalized_url,
      visibility: "private",
      auth_requirement: "required",
      auth_status: "unsupported",
      supports_interactive_login: detection.supports_interactive_login,
      message:
        "Private repository selected, but HGP could not identify a supported login platform from this URL. Only public repositories are supported for this host right now.",
    };
  }

  return {
    provider_id: detection.provider_id,
    provider_label: detection.provider_label,
    instance_url: detection.instance_url,
    normalized_url: detection.normalized_url,
    visibility: "private",
    auth_requirement: "required",
    auth_status: "unsupported",
    supports_interactive_login: detection.supports_interactive_login,
    message: `Private ${detection.provider_label} repositories are not supported yet. Only public repositories are available for this platform right now.`,
  };
}

function buildRepositoryCredentialOptions(
  credentials: SecretCredentialSetting[],
  repositoryCredentialId: number | null,
  isLoadingRepositoryCredentials: boolean,
): SelectOption[] {
  const placeholderLabel = isLoadingRepositoryCredentials
    ? "Loading stored credentials..."
    : credentials.length === 0
      ? "No stored repository credentials available"
      : "No repository credential selected";
  const options: SelectOption[] = [
    {
      disabled: isLoadingRepositoryCredentials,
      label: placeholderLabel,
      value: "",
    },
    ...credentials.map((credential) => ({
      label: formatRepositoryCredentialOptionLabel(credential),
      value: credential.credential_id.toString(),
    })),
  ];

  if (
    repositoryCredentialId !== null &&
    !credentials.some(
      (credential) => credential.credential_id === repositoryCredentialId,
    )
  ) {
    options.push({
      label: `Current credential #${repositoryCredentialId}`,
      value: repositoryCredentialId.toString(),
    });
  }

  return options;
}

function formatRepositoryCredentialOptionLabel(
  credential: SecretCredentialSetting,
) {
  return `${credential.name} (${formatRepositoryCredentialKindLabel(credential.kind)})`;
}

function formatRepositoryCredentialKindLabel(kind: string) {
  switch (kind) {
    case "git-http-basic":
      return "HTTP basic";
    case "git-http-bearer":
      return "Bearer token";
    case "git-http-github-host-login":
      return "GitHub login";
    default:
      return kind;
  }
}

function isRepositoryCredentialSelectable(
  credential: SecretCredentialSetting,
) {
  return (
    credential.config_summary.status === "ready" &&
    [
      "git-http-basic",
      "git-http-bearer",
      "git-http-github-host-login",
    ].includes(credential.kind)
  );
}

function isItchCredentialSelectable(credential: SecretCredentialSetting) {
  return (
    credential.kind === "itch-api-key" &&
    credential.config_summary.status === "ready"
  );
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
