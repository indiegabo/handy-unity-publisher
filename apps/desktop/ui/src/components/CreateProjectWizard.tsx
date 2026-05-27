import {
  startTransition,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";

import { Button } from "./Button";
import {
  BuildTargetEditorOverlay,
  type BuildTargetEditorOverlayResult,
  type SharedBuildTargetDraft,
} from "./BuildTargetEditorOverlay";
import { BuildTargetRemovalCallout } from "./BuildTargetRemovalCallout";
import {
  PublishDestinationsEditor,
  buildCreateProjectPublishTargetsInput,
  collectBuildTargetBindingImpact,
  hasPublishDestinationValidationErrors,
  removeBuildTargetBindings,
  type ProjectBuildTargetReference,
  type PublishDestinationDraft,
  validatePublishDestinationDrafts,
} from "./PublishDestinationsEditor";
import { type SelectOption } from "./Field";
import { REPOSITORY_ENGINE_OPTIONS } from "./RepositoryEngineField";
import { FocusPageFrame, MetaItem, MetaRow, SurfacePanel } from "./Surface";
import { useOverlay } from "./OverlayManager";
import { type AuthProviderConnectionResult } from "./authProviderPresentation";
import {
  ProjectIdentityStep,
  ProjectLocalAccessStep,
  ProjectPathsStep,
  ProjectPublishStep,
  ProjectRepositoryAccessPanel,
  ProjectRepositoryAccessStep,
  ProjectReviewStep,
  ProjectTargetsStep,
} from "./wizard/ProjectDefinitionSteps";
import {
  createRepositoryProject,
  detectRepositoryProvider,
  loadSecretSettings,
  loadRepositoryInspection,
  loadUnityAdapterSettings,
  saveSecretCredential,
  validateUnityExecutablePath,
  type CreateRepositoryProjectInput,
  type DiscoveredUnityEditor,
  type RepositoryAccessAssessment,
  type RepositoryEngineKind,
  type RepositoryProviderDetection,
  type SaveSecretCredentialInput,
  type RepositoryInspectionEntry,
  type SecretCredentialSetting,
  type UnityAdapterSettings,
  type UnityExecutableValidation,
} from "../services/projects";
import {
  loadAuthProviders,
  loginWithGithubAuth,
  type AuthProviderStatus,
} from "../services/auth";

export type BuildTargetDraft = {
  id: string;
  name: string;
  targetPlatform: string;
  buildMethod: string;
};

export type ProjectDraft = {
  projectKind: "repository" | "local";
  engineKind: RepositoryEngineKind;
  name: string;
  repositoryUrl: string;
  localPath: string;
  repositoryVisibility: "public" | "private";
  pollingIntervalSeconds: string;
  artifactsRootOverride: string;
  workspaceRootOverride: string;
  unityExecutablePath: string;
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
};

type TargetStepErrors = {
  root?: string;
  targets: Record<string, TargetFieldErrors>;
};

type PathStepErrors = {
  artifactsRootOverride?: string;
  workspaceRootOverride?: string;
};

export type CreateProjectWizardSnapshot = {
  attemptedSteps: Record<WizardStepKey, boolean>;
  currentStepIndex: number;
  draft: ProjectDraft;
  expandedTargetIds: Record<string, boolean>;
  unityExecutableDiagnostics: UnityExecutableValidation | null;
  pendingBuildTargetRemovalId: string | null;
  repositoryCredentialId: number | null;
  touchedFields: Record<string, boolean>;
};

type ProjectPathFieldName =
  | "localPath"
  | "artifactsRootOverride"
  | "workspaceRootOverride";

type CreateProjectWizardProps = {
  authProviderResult?: AuthProviderConnectionResult | null;
  initialSnapshot?: CreateProjectWizardSnapshot | null;
  onCreated: (repositoryId: number) => void;
  onDirtyChange?: (isDirty: boolean) => void;
  onManageAuth?: () => void;
  onRequestClose?: () => void;
  onSnapshotChange?: (snapshot: CreateProjectWizardSnapshot) => void;
};

type WizardStepDefinition = {
  key: WizardStepKey;
  label: string;
  description: string;
};

type ProjectSourceWizardAdapter = {
  kind: "repository" | "local";
  stepLabel: string;
  stepDescription: string;
  supportTitle: string;
  supportDescription: string;
  supportCopy: string;
  unsupportedMessage: string | null;
};

type BuildTargetWizardAdapter = {
  kind: "unity" | "engine-unsupported";
  stepLabel: string;
  stepDescription: string;
  supportTitle: string;
  supportDescription: string;
  supportCopy: string;
  reviewDescription: string;
  unsupportedMessage: string | null;
};

const WIZARD_STEP_ORDER: readonly WizardStepKey[] = [
  "identity",
  "access",
  "targets",
  "publish",
  "paths",
  "review",
];

const PROJECT_KIND_OPTIONS = [
  { label: "Repository project", value: "repository" },
  { label: "Local workspace project", value: "local" },
] as const;

const REPOSITORY_VISIBILITY_OPTIONS = [
  { label: "Public", value: "public" },
  { label: "Private", value: "private" },
] as const;

const EMPTY_VALIDATION_ATTEMPTS: Record<WizardStepKey, boolean> = {
  identity: false,
  access: false,
  targets: false,
  publish: false,
  paths: false,
  review: false,
};

const INITIAL_PROJECT_DRAFT = createInitialProjectDraft();
const INITIAL_PROJECT_DRAFT_DIRTY_KEY = buildProjectDraftDirtyKey(
  INITIAL_PROJECT_DRAFT,
);

export function CreateProjectWizard({
  authProviderResult = null,
  initialSnapshot: initialSnapshotProp = null,
  onCreated,
  onDirtyChange,
  onManageAuth,
  onRequestClose,
  onSnapshotChange,
}: CreateProjectWizardProps) {
  const initialSnapshot =
    initialSnapshotProp ?? createInitialCreateProjectWizardSnapshot();
  const { openOverlay } = useOverlay();
  const [draft, setDraft] = useState<ProjectDraft>(() =>
    cloneProjectDraft(initialSnapshot.draft),
  );
  const [currentStepIndex, setCurrentStepIndex] = useState(() =>
    normalizeWizardStepIndex(initialSnapshot.currentStepIndex),
  );
  const [attemptedSteps, setAttemptedSteps] = useState(() => ({
    ...EMPTY_VALIDATION_ATTEMPTS,
    ...initialSnapshot.attemptedSteps,
  }));
  const [touchedFields, setTouchedFields] = useState<Record<string, boolean>>(
    () => ({
      ...initialSnapshot.touchedFields,
    }),
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
  >(initialSnapshot.repositoryCredentialId);
  const [repositoryAccessActionMessage, setRepositoryAccessActionMessage] =
    useState<string | null>(null);
  const [pendingRepositoryAccessAction, setPendingRepositoryAccessAction] =
    useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [unityAdapterSettings, setUnityAdapterSettings] =
    useState<UnityAdapterSettings | null>(null);
  const [isLoadingUnityAdapterSettings, setIsLoadingUnityAdapterSettings] =
    useState(true);
  const [unityAdapterSettingsError, setUnityAdapterSettingsError] = useState<
    string | null
  >(null);
  const [unityExecutableDiagnostics, setUnityExecutableDiagnostics] =
    useState<UnityExecutableValidation | null>(
      initialSnapshot.unityExecutableDiagnostics,
    );
  const [isValidatingUnityExecutable, setIsValidatingUnityExecutable] =
    useState(false);
  const [expandedTargetIds, setExpandedTargetIds] = useState<
    Record<string, boolean>
  >(() => {
    if (Object.keys(initialSnapshot.expandedTargetIds).length > 0) {
      return {
        ...initialSnapshot.expandedTargetIds,
      };
    }

    return {
      "target-1": true,
    };
  });
  const [pendingBuildTargetRemovalId, setPendingBuildTargetRemovalId] =
    useState<string | null>(initialSnapshot.pendingBuildTargetRemovalId);
  const nextBuildTargetIdRef = useRef(
    resolveNextBuildTargetIndex(initialSnapshot.draft.buildTargets),
  );
  const unityExecutableValidationTimerRef = useRef<number | undefined>(
    undefined,
  );
  const unityExecutableValidationTokenRef = useRef(0);
  const accessAssessmentTimerRef = useRef<number | undefined>(undefined);
  const accessAssessmentTokenRef = useRef(0);

  const projectSourceStepAdapter = resolveProjectSourceWizardAdapter(
    draft.projectKind,
  );
  const buildTargetStepAdapter = resolveBuildTargetWizardAdapter(
    draft.engineKind,
    draft.projectKind,
  );
  const wizardSteps = buildWizardSteps(
    projectSourceStepAdapter,
    buildTargetStepAdapter,
  );
  const currentStep = wizardSteps[currentStepIndex];
  const currentStepNumber = currentStepIndex + 1;
  const showPreviousAction = currentStepIndex > 0;
  const showNextAction = currentStep.key !== "review";
  const identityErrors = validateIdentityStep(draft, repositoryInventory);
  const accessErrors = validateAccessStep(
    draft,
    repositoryInventory,
    {
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
    },
    projectSourceStepAdapter,
  );
  const targetErrors = validateTargetsStep(
    draft,
    unityExecutableDiagnostics,
    isValidatingUnityExecutable,
    buildTargetStepAdapter,
  );
  const buildTargetReferences: ProjectBuildTargetReference[] =
    draft.buildTargets.map((target) => ({
      id: target.id,
      buildTargetId: null,
      name: target.name.trim() || "Unnamed target",
    }));
  const publishDestinationErrors = validatePublishDestinationDrafts(
    draft.publishDestinations,
    buildTargetReferences,
  );
  const pendingBuildTargetRemoval = pendingBuildTargetRemovalId
    ? (draft.buildTargets.find(
        (target) => target.id === pendingBuildTargetRemovalId,
      ) ?? null)
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
  const discoveredUnityEditors =
    listSelectableUnityEditors(unityAdapterSettings);
  const currentStepBlockedByInventory =
    (currentStep.key === "identity" || currentStep.key === "access") &&
    (isLoadingRepositoryInventory || inventoryError !== null);
  const isWizardDirty = buildCreateProjectWizardDirtyState(
    draft,
    currentStepIndex,
    repositoryCredentialId,
  );

  useEffect(() => {
    onDirtyChange?.(isWizardDirty);
  }, [isWizardDirty, onDirtyChange]);

  useEffect(() => {
    if (!onSnapshotChange) {
      return;
    }

    onSnapshotChange({
      attemptedSteps,
      currentStepIndex,
      draft: cloneProjectDraft(draft),
      expandedTargetIds: {
        ...expandedTargetIds,
      },
      unityExecutableDiagnostics,
      pendingBuildTargetRemovalId,
      repositoryCredentialId,
      touchedFields: {
        ...touchedFields,
      },
    });
  }, [
    attemptedSteps,
    currentStepIndex,
    draft,
    expandedTargetIds,
    onSnapshotChange,
    unityExecutableDiagnostics,
    pendingBuildTargetRemovalId,
    repositoryCredentialId,
    touchedFields,
  ]);

  const loadRepositoryInventoryEffect = useEffectEvent(async () => {
    setIsLoadingRepositoryInventory(true);
    setInventoryError(null);

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
    setAuthProviderError(null);

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

  const loadUnityAdapterSettingsEffect = useEffectEvent(async () => {
    setIsLoadingUnityAdapterSettings(true);
    setUnityAdapterSettingsError(null);

    try {
      const settings = await loadUnityAdapterSettings();

      startTransition(() => {
        setUnityAdapterSettings(settings);
        setUnityAdapterSettingsError(null);
        setIsLoadingUnityAdapterSettings(false);
      });
    } catch (error) {
      startTransition(() => {
        setUnityAdapterSettings(null);
        setUnityAdapterSettingsError(buildProjectErrorMessage(error));
        setIsLoadingUnityAdapterSettings(false);
      });
    }
  });

  const loadRepositoryCredentialsEffect = useEffectEvent(async () => {
    setIsLoadingRepositoryCredentials(true);
    setRepositoryCredentialsError(null);

    try {
      const credentials = await listRepositoryCredentialsEffect();

      startTransition(() => {
        setRepositoryCredentials(
          credentials.filter(isRepositoryCredentialSelectable),
        );
        setPublishCredentials(
          credentials.filter(
            (credential) => credential.kind === "itch-api-key",
          ),
        );
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
        setSubmitError(null);
      });
    },
  );

  const handleSavePublishCredential = useEffectEvent(
    async (destinationId: string, input: SaveSecretCredentialInput) => {
      try {
        const createdCredentialId = await saveSecretCredential(input);
        const credentials = await listRepositoryCredentialsEffect();
        const createdCredential = credentials.find(
          (credential) => credential.credential_id === createdCredentialId,
        );
        const credentialName = createdCredential?.name || input.name.trim();

        startTransition(() => {
          setRepositoryCredentials(
            credentials.filter(isRepositoryCredentialSelectable),
          );
          setPublishCredentials(
            credentials.filter(
              (credential) => credential.kind === "itch-api-key",
            ),
          );
          setDraft((current) => ({
            ...current,
            publishDestinations: current.publishDestinations.map(
              (destination) =>
                destination.id === destinationId
                  ? {
                      ...destination,
                      credentialsId: createdCredentialId,
                      credentialsName: credentialName,
                    }
                  : destination,
            ),
          }));
        });

        return createdCredentialId;
      } catch (error) {
        throw new Error(buildProjectErrorMessage(error));
      }
    },
  );

  useEffect(() => {
    void loadRepositoryInventoryEffect();
    void loadAuthProvidersEffect();
    void loadRepositoryCredentialsEffect();
    void loadUnityAdapterSettingsEffect();

    return () => {
      if (accessAssessmentTimerRef.current !== undefined) {
        window.clearTimeout(accessAssessmentTimerRef.current);
      }
      if (unityExecutableValidationTimerRef.current !== undefined) {
        window.clearTimeout(unityExecutableValidationTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (buildTargetStepAdapter.kind !== "unity") {
      return;
    }

    const unityExecutablePath = draft.unityExecutablePath.trim();
    if (
      !unityExecutablePath ||
      unityExecutableDiagnostics ||
      isValidatingUnityExecutable
    ) {
      return;
    }

    scheduleUnityExecutableValidation(unityExecutablePath, 0);
  }, [
    buildTargetStepAdapter.kind,
    draft.unityExecutablePath,
    unityExecutableDiagnostics,
    isValidatingUnityExecutable,
  ]);

  useEffect(() => {
    if (draft.projectKind !== "repository") {
      accessAssessmentTokenRef.current += 1;

      if (accessAssessmentTimerRef.current !== undefined) {
        window.clearTimeout(accessAssessmentTimerRef.current);
        accessAssessmentTimerRef.current = undefined;
      }

      startTransition(() => {
        setRepositoryAccessAssessment(null);
        setRepositoryAccessError(null);
        setIsAssessingRepositoryAccess(false);
        setRepositoryAccessActionMessage(null);
        setRepositoryCredentialId(null);
      });
      return;
    }

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
  }, [draft.projectKind, draft.repositoryUrl, draft.repositoryVisibility]);

  useEffect(() => {
    if (
      !repositoryAccessAssessment ||
      supportsShellRepositoryLoginAction(repositoryAccessAssessment)
    ) {
      return;
    }

    startTransition(() => {
      setRepositoryCredentialId(null);
    });
  }, [repositoryAccessAssessment]);

  useEffect(() => {
    if (
      !authProviderResult ||
      !isGithubAuthProviderResult(authProviderResult)
    ) {
      return;
    }

    startTransition(() => {
      setGithubAuthProvider(authProviderResult.provider);
      setAuthProviderError(null);
      setRepositoryAccessActionMessage(
        buildAuthProviderRoundTripMessage(
          authProviderResult,
          repositoryCredentialId,
          githubAuthProvider?.credential_id ?? null,
        ),
      );

      if (
        authProviderResult.provider.credential_id !== null &&
        (repositoryCredentialId === null ||
          repositoryCredentialId ===
            (githubAuthProvider?.credential_id ?? null))
      ) {
        setRepositoryCredentialId(authProviderResult.provider.credential_id);
      }
    });
  }, [
    authProviderResult,
    githubAuthProvider?.credential_id,
    repositoryCredentialId,
  ]);

  const handleRetryRepositoryInventory = useEffectEvent(() => {
    void loadRepositoryInventoryEffect();
  });

  const handleRetryAuthProviders = useEffectEvent(() => {
    void loadAuthProvidersEffect();
  });

  const handleRetryRepositoryCredentials = useEffectEvent(() => {
    void loadRepositoryCredentialsEffect();
  });

  const handleRetryRepositoryAccessCheck = useEffectEvent(() => {
    if (draft.projectKind !== "repository") {
      return;
    }

    const normalizedUrl = draft.repositoryUrl.trim();

    if (
      !normalizedUrl ||
      !(
        normalizedUrl.startsWith("https://") ||
        normalizedUrl.startsWith("http://")
      )
    ) {
      return;
    }

    if (accessAssessmentTimerRef.current !== undefined) {
      window.clearTimeout(accessAssessmentTimerRef.current);
      accessAssessmentTimerRef.current = undefined;
    }

    accessAssessmentTokenRef.current += 1;
    const assessmentToken = accessAssessmentTokenRef.current;

    startTransition(() => {
      setIsAssessingRepositoryAccess(true);
      setRepositoryAccessError(null);
      setRepositoryAccessActionMessage(null);
    });

    void loadRepositoryAccessAssessmentEffect(
      normalizedUrl,
      draft.repositoryVisibility,
      assessmentToken,
    );
  });

  const markFieldTouched = useEffectEvent((fieldKey: string) => {
    startTransition(() => {
      setTouchedFields((current) => ({
        ...current,
        [fieldKey]: true,
      }));
    });
  });

  const scheduleUnityExecutableValidation = useEffectEvent(
    (path: string, delayMillis = 250) => {
      if (unityExecutableValidationTimerRef.current !== undefined) {
        window.clearTimeout(unityExecutableValidationTimerRef.current);
      }

      unityExecutableValidationTokenRef.current += 1;
      const validationToken = unityExecutableValidationTokenRef.current;
      const trimmedPath = path.trim();

      if (!trimmedPath) {
        startTransition(() => {
          setUnityExecutableDiagnostics(null);
          setIsValidatingUnityExecutable(false);
        });
        return;
      }

      startTransition(() => {
        setIsValidatingUnityExecutable(true);
      });

      unityExecutableValidationTimerRef.current = window.setTimeout(
        async () => {
          try {
            const diagnostics = await validateUnityExecutablePath(trimmedPath);
            if (unityExecutableValidationTokenRef.current !== validationToken) {
              return;
            }

            startTransition(() => {
              setUnityExecutableDiagnostics(diagnostics);
              setIsValidatingUnityExecutable(false);
            });
          } catch (error) {
            if (unityExecutableValidationTokenRef.current !== validationToken) {
              return;
            }

            startTransition(() => {
              setUnityExecutableDiagnostics({
                runner_family: "host-native",
                unity_executable_path: trimmedPath,
                unity_executable_exists: false,
                unity_executable_is_file: false,
                additional_argument_count: 0,
                environment_variable_count: 0,
                status: "validation_failed",
                message: buildProjectErrorMessage(error),
              });
              setIsValidatingUnityExecutable(false);
            });
          }
        },
        delayMillis,
      );
    },
  );

  const handlePathPickerError = useEffectEvent((error: unknown) => {
    startTransition(() => {
      setSubmitError(buildProjectErrorMessage(error));
    });
  });

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

  const handleAddBuildTarget = useEffectEvent(async () => {
    const targetId = `target-${nextBuildTargetIdRef.current}`;
    const created = await openOverlay<BuildTargetEditorOverlayResult>(
      BuildTargetEditorOverlay,
      {
        initialTarget: createEmptyBuildTargetDraft(
          nextBuildTargetIdRef.current,
        ),
        mode: "create",
        targetId,
      },
    );

    if (!created) {
      return;
    }

    nextBuildTargetIdRef.current += 1;

    startTransition(() => {
      setDraft((current) => ({
        ...current,
        buildTargets: [
          ...current.buildTargets,
          toWizardBuildTargetDraft(created.target),
        ],
      }));
    });
  });

  const handleEditBuildTarget = useEffectEvent(async (targetId: string) => {
    const target = draft.buildTargets.find((entry) => entry.id === targetId);
    if (!target) {
      return;
    }

    const updated = await openOverlay<BuildTargetEditorOverlayResult>(
      BuildTargetEditorOverlay,
      {
        initialTarget: target,
        mode: "edit",
        targetId,
      },
    );

    if (!updated) {
      return;
    }

    startTransition(() => {
      setDraft((current) => ({
        ...current,
        buildTargets: current.buildTargets.map((entry) =>
          entry.id === targetId
            ? toWizardBuildTargetDraft(updated.target)
            : entry,
        ),
      }));
    });
  });

  const finalizeBuildTargetRemoval = useEffectEvent((targetId: string) => {
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
      setExpandedTargetIds((current) => {
        const next = { ...current };
        delete next[targetId];
        return next;
      });
    });
  });

  const handleRemoveBuildTarget = useEffectEvent((targetId: string) => {
    if (
      collectBuildTargetBindingImpact(draft.publishDestinations, targetId)
        .length > 0
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

  const handleAdvanceStep = useEffectEvent(() => {
    if (currentStep.key === "identity") {
      if (
        hasIdentityErrors(identityErrors) ||
        isLoadingRepositoryInventory ||
        inventoryError !== null
      ) {
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
      if (
        hasAccessErrors(accessErrors) ||
        isLoadingRepositoryInventory ||
        inventoryError !== null
      ) {
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
        Math.min(current + 1, WIZARD_STEP_ORDER.length - 1),
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
      inventoryError,
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

  const repositoryAccessPanel =
    currentStep.key === "access" && projectSourceStepAdapter.kind === "repository" ? (
      <ProjectRepositoryAccessPanel
        authProviderError={authProviderError}
        githubAuthProvider={githubAuthProvider}
        isAssessingRepositoryAccess={isAssessingRepositoryAccess}
        isLoadingAuthProviders={isLoadingAuthProviders}
        isLoadingRepositoryCredentials={isLoadingRepositoryCredentials}
        onBindRepositoryAccess={() => {
          void handleBindRepositoryAccess();
        }}
        onClearRepositoryAccessBinding={handleClearRepositoryAccessBinding}
        onManageAuth={onManageAuth}
        onRepositoryCredentialChange={handleRepositoryCredentialSelectionChange}
        onRetryAuthProviders={handleRetryAuthProviders}
        onRetryRepositoryAccessCheck={handleRetryRepositoryAccessCheck}
        onRetryRepositoryCredentials={handleRetryRepositoryCredentials}
        pendingRepositoryAccessAction={pendingRepositoryAccessAction}
        repositoryAccessActionMessage={repositoryAccessActionMessage}
        repositoryAccessAssessment={repositoryAccessAssessment}
        repositoryAccessError={repositoryAccessError}
        repositoryCredentialId={repositoryCredentialId}
        repositoryCredentialOptions={repositoryCredentialOptions}
        repositoryCredentialsError={repositoryCredentialsError}
        repositoryUrl={draft.repositoryUrl}
        validationError={
          shouldShowFieldError(
            attemptedSteps.access,
            touchedFields,
            "repositoryAccess",
          )
            ? accessErrors.repositoryAccess
            : undefined
        }
      />
    ) : null;

  const wizardStageContentClassName = "wizard-stage-content-shell";
  const stagePanelClassName = joinClassNames(
    "wizard-stage-panel",
    currentStep.key === "targets" && "wizard-stage-panel--full-bleed",
  );
  const stagePanelTone = currentStep.key === "targets" ? "ghost" : "section";

  return (
    <FocusPageFrame
      bodyClassName="wizard-shell__body"
      className="wizard-shell"
      eyebrow="Create Project"
      summary={
        <MetaRow>
          <MetaItem label="Mode">
            {formatProjectKindLabel(draft.projectKind)}
          </MetaItem>
          <MetaItem label="Engine">
            {formatRepositoryEngineKindLabel(draft.engineKind)}
          </MetaItem>
          <MetaItem label="Targets">
            {formatWizardTargetCount(draft.buildTargets.length)}
          </MetaItem>
        </MetaRow>
      }
    >
      {inventoryError || submitError ? (
        <div className="wizard-shell__messages">
          {inventoryError ? (
            <>
              <p className="feed-banner feed-banner--error">{inventoryError}</p>
              <div className="wizard-callout__actions">
                <Button
                  disabled={isLoadingRepositoryInventory}
                  leadingIcon="refresh"
                  onClick={handleRetryRepositoryInventory}
                  size="sm"
                  variant="secondary"
                >
                  {isLoadingRepositoryInventory
                    ? "Retrying inventory..."
                    : "Retry inventory load"}
                </Button>
              </div>
            </>
          ) : null}

          {submitError ? (
            <p className="feed-banner feed-banner--error">{submitError}</p>
          ) : null}
        </div>
      ) : null}

      <section className="wizard-layout">
        <header aria-label="Wizard status" className="wizard-layout__header">
          <div className="wizard-layout__header-main">
            <p className="wizard-layout__step-label">
              {`${currentStepNumber}. ${currentStep.label}`}
            </p>
            <span aria-hidden="true" className="wizard-layout__separator">
              |
            </span>
            <p className="wizard-layout__step-count">
              {`${currentStepNumber} of ${wizardSteps.length}`}
            </p>
          </div>
        </header>

        <div className="wizard-layout__body">
          <div className={wizardStageContentClassName}>
            <SurfacePanel className={stagePanelClassName} tone={stagePanelTone}>
              {currentStep.key === "identity" ? (
                <ProjectIdentityStep
                  draft={draft}
                  errors={{
                    engineKind: shouldShowFieldError(
                      attemptedSteps.identity,
                      touchedFields,
                      "engineKind",
                    )
                      ? identityErrors.engineKind
                      : undefined,
                    name: shouldShowFieldError(
                      attemptedSteps.identity,
                      touchedFields,
                      "name",
                    )
                      ? identityErrors.name
                      : undefined,
                    projectKind: shouldShowFieldError(
                      attemptedSteps.identity,
                      touchedFields,
                      "projectKind",
                    )
                      ? identityErrors.projectKind
                      : undefined,
                  }}
                  onEngineKindChange={(engineKind) => {
                    startTransition(() => {
                      setDraft((current) => ({
                        ...current,
                        engineKind,
                      }));
                    });
                    markFieldTouched("engineKind");
                  }}
                  onFieldBlur={markFieldTouched}
                  onNameChange={(nextValue) => {
                    startTransition(() => {
                      setDraft((current) => ({
                        ...current,
                        name: nextValue,
                      }));
                    });
                    markFieldTouched("name");
                  }}
                  onProjectKindChange={(projectKind) => {
                    startTransition(() => {
                      setDraft((current) => ({
                        ...current,
                        projectKind,
                      }));
                    });
                    markFieldTouched("projectKind");
                  }}
                />
              ) : null}

              {currentStep.key === "access" ? (
                projectSourceStepAdapter.kind === "repository" ? (
                  <ProjectRepositoryAccessStep
                    onPollingIntervalSecondsBlur={() =>
                      markFieldTouched("pollingIntervalSeconds")
                    }
                    onPollingIntervalSecondsChange={(nextValue) => {
                      startTransition(() => {
                        setDraft((current) => ({
                          ...current,
                          pollingIntervalSeconds: nextValue,
                        }));
                      });
                      markFieldTouched("pollingIntervalSeconds");
                    }}
                    onRepositoryUrlBlur={() => {
                      markFieldTouched("repositoryUrl");
                      markFieldTouched("repositoryAccess");
                    }}
                    onRepositoryUrlChange={(nextValue) => {
                      startTransition(() => {
                        setDraft((current) => ({
                          ...current,
                          repositoryUrl: nextValue,
                        }));
                      });
                      markFieldTouched("repositoryUrl");
                      markFieldTouched("repositoryAccess");
                    }}
                    onRepositoryVisibilityBlur={() =>
                      markFieldTouched("repositoryAccess")
                    }
                    onRepositoryVisibilityChange={(nextValue) => {
                      startTransition(() => {
                        setDraft((current) => ({
                          ...current,
                          repositoryVisibility: nextValue,
                        }));
                      });
                      markFieldTouched("repositoryAccess");
                    }}
                    pollingIntervalSeconds={draft.pollingIntervalSeconds}
                    pollingIntervalSecondsError={
                      shouldShowFieldError(
                        attemptedSteps.access,
                        touchedFields,
                        "pollingIntervalSeconds",
                      )
                        ? accessErrors.pollingIntervalSeconds
                        : undefined
                    }
                    repositoryAccessPanel={repositoryAccessPanel}
                    repositoryUrl={draft.repositoryUrl}
                    repositoryUrlError={
                      shouldShowFieldError(
                        attemptedSteps.access,
                        touchedFields,
                        "repositoryUrl",
                      )
                        ? accessErrors.repositoryUrl
                        : undefined
                    }
                    repositoryVisibility={draft.repositoryVisibility}
                  />
                ) : (
                  <ProjectLocalAccessStep
                    disabled={isSubmitting}
                    localPath={draft.localPath}
                    localPathError={
                      shouldShowFieldError(
                        attemptedSteps.access,
                        touchedFields,
                        "localPath",
                      )
                        ? accessErrors.localPath
                        : undefined
                    }
                    onClearLocalPath={() => handleProjectPathCleared("localPath")}
                    onPathPickError={handlePathPickerError}
                    onPathPicked={(selectedPath) =>
                      handleProjectPathPicked("localPath", selectedPath)
                    }
                  />
                )
              ) : null}

              {currentStep.key === "targets" ? (
                <ProjectTargetsStep
                  buildTargetAdapter={buildTargetStepAdapter}
                  buildTargets={draft.buildTargets}
                  detectedEditorDisabled={
                    isLoadingUnityAdapterSettings ||
                    discoveredUnityEditors.length === 0
                  }
                  detectedEditorHint={buildDetectedUnityEditorHint(
                    unityAdapterSettingsError,
                    discoveredUnityEditors.length,
                  )}
                  detectedEditorOptions={buildDetectedUnityEditorOptions(
                    discoveredUnityEditors,
                    isLoadingUnityAdapterSettings,
                    unityAdapterSettingsError,
                  )}
                  detectedEditorValue={resolveDetectedUnityEditorValue(
                    draft.unityExecutablePath,
                    discoveredUnityEditors,
                  )}
                  isBusy={isSubmitting}
                  isValidatingUnityExecutable={isValidatingUnityExecutable}
                  onAddTarget={() => {
                    void handleAddBuildTarget();
                  }}
                  onDetectedEditorChange={(selectedPath) => {
                    startTransition(() => {
                      setDraft((current) => ({
                        ...current,
                        unityExecutablePath: selectedPath,
                      }));
                      setUnityExecutableDiagnostics(null);
                    });
                    scheduleUnityExecutableValidation(selectedPath);
                  }}
                  onEditTarget={(targetId) => {
                    void handleEditBuildTarget(targetId);
                  }}
                  onRemoveTarget={handleRemoveBuildTarget}
                  onUnityExecutablePickError={handlePathPickerError}
                  onUnityExecutablePicked={(selectedPath) => {
                    startTransition(() => {
                      setDraft((current) => ({
                        ...current,
                        unityExecutablePath: selectedPath,
                      }));
                      setUnityExecutableDiagnostics(null);
                    });
                    scheduleUnityExecutableValidation(selectedPath);
                  }}
                  removalCallout={
                    pendingBuildTargetRemoval ? (
                      <BuildTargetRemovalCallout
                        bindingImpact={pendingBuildTargetBindingImpact}
                        onCancel={() => setPendingBuildTargetRemovalId(null)}
                        onConfirm={handleConfirmBuildTargetRemoval}
                        targetName={pendingBuildTargetRemoval.name}
                      />
                    ) : null
                  }
                  rootError={
                    shouldShowStepError(attemptedSteps.targets)
                      ? targetErrors.root
                      : undefined
                  }
                  targetErrors={
                    shouldShowStepError(attemptedSteps.targets)
                      ? targetErrors.targets
                      : {}
                  }
                  unityExecutableDiagnostics={unityExecutableDiagnostics}
                  unityExecutableError={
                    shouldShowStepError(attemptedSteps.targets)
                      ? targetErrors.root
                      : undefined
                  }
                  unityExecutablePath={draft.unityExecutablePath}
                />
              ) : null}

              {currentStep.key === "publish" ? (
                <ProjectPublishStep
                  buildTargets={buildTargetReferences}
                  credentials={publishCredentials}
                  destinations={draft.publishDestinations}
                  disabled={isSubmitting}
                  errors={
                    attemptedSteps.publish ? publishDestinationErrors : undefined
                  }
                  onChange={(nextPublishDestinations) => {
                    startTransition(() => {
                      setDraft((current) => ({
                        ...current,
                        publishDestinations: nextPublishDestinations,
                      }));
                    });
                  }}
                  onSaveCredential={handleSavePublishCredential}
                  showItchUserversionTemplate={draft.projectKind === "repository"}
                />
              ) : null}

              {currentStep.key === "paths" ? (
                <ProjectPathsStep
                  artifactsRootOverride={draft.artifactsRootOverride}
                  artifactsRootOverrideError={
                    shouldShowFieldError(
                      attemptedSteps.paths,
                      touchedFields,
                      "artifactsRootOverride",
                    )
                      ? pathErrors.artifactsRootOverride
                      : undefined
                  }
                  disabled={isSubmitting}
                  onArtifactsRootClear={() =>
                    handleProjectPathCleared("artifactsRootOverride")
                  }
                  onArtifactsRootPicked={(selectedPath) =>
                    handleProjectPathPicked("artifactsRootOverride", selectedPath)
                  }
                  onPathPickError={handlePathPickerError}
                  onWorkspaceRootClear={() =>
                    handleProjectPathCleared("workspaceRootOverride")
                  }
                  onWorkspaceRootPicked={(selectedPath) =>
                    handleProjectPathPicked("workspaceRootOverride", selectedPath)
                  }
                  workspaceRootOverride={draft.workspaceRootOverride}
                  workspaceRootOverrideError={
                    shouldShowFieldError(
                      attemptedSteps.paths,
                      touchedFields,
                      "workspaceRootOverride",
                    )
                      ? pathErrors.workspaceRootOverride
                      : undefined
                  }
                />
              ) : null}

              {currentStep.key === "review" ? (
                <ProjectReviewStep
                  buildTargetAdapter={buildTargetStepAdapter}
                  draft={draft}
                  isValidatingUnityExecutable={isValidatingUnityExecutable}
                  projectSourceStepAdapter={projectSourceStepAdapter}
                  repositoryAccessSummary={repositoryAccessSummary}
                  unityExecutableDiagnostics={unityExecutableDiagnostics}
                />
              ) : null}
            </SurfacePanel>
          </div>
        </div>

        <footer className="wizard-footer">
          <div className="wizard-footer__slot wizard-footer__slot--start">
            {onRequestClose ? (
              <Button
                disabled={isSubmitting}
                onClick={onRequestClose}
                size="sm"
                variant="ghost"
              >
                Cancel
              </Button>
            ) : null}

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
                disabled={isSubmitting || currentStepBlockedByInventory}
                onClick={handleAdvanceStep}
                size="sm"
                variant="primary"
              >
                Next
              </Button>
            ) : null}
          </div>
        </footer>
      </section>
    </FocusPageFrame>
  );
}

function formatProjectKindLabel(projectKind: ProjectDraft["projectKind"]) {
  return projectKind === "repository"
    ? "Repository project"
    : "Local workspace project";
}

function formatRepositoryEngineKindLabel(engineKind: RepositoryEngineKind) {
  const option = REPOSITORY_ENGINE_OPTIONS.find(
    (entry) => entry.value === engineKind,
  );

  if (option) {
    return option.label;
  }

  return engineKind
    .split("-")
    .map((segment) =>
      segment.length > 0
        ? `${segment[0].toUpperCase()}${segment.slice(1)}`
        : segment,
    )
    .join(" ");
}

function formatWizardTargetCount(targetCount: number) {
  return `${targetCount} target${targetCount === 1 ? "" : "s"}`;
}

function formatProjectSourceAdapterStatus(adapter: ProjectSourceWizardAdapter) {
  return adapter.kind === "repository" || adapter.kind === "local"
    ? "Available"
    : "Unavailable";
}

function resolveProjectSourceWizardAdapter(
  projectKind: ProjectDraft["projectKind"],
): ProjectSourceWizardAdapter {
  if (projectKind === "repository") {
    return {
      kind: "repository",
      stepLabel: "Repository",
      stepDescription:
        "Declare where HGP should sync this project, authenticate, and watch for changes.",
      supportTitle: "Repository source",
      supportDescription:
        "Repository-backed projects rely on the source adapter to detect providers, credentials, and polling posture.",
      supportCopy:
        "This source adapter lets the runtime poll a remote repository, assess access, and queue automation from new releases.",
      unsupportedMessage: null,
    };
  }

  return {
    kind: "local",
    stepLabel: "Workspace",
    stepDescription:
      "Declare the local workspace source that HGP should manage for this project.",
    supportTitle: "Local workspace source",
    supportDescription:
      "Local workspace projects point HGP at one host path that should be released without a managed repository checkout.",
    supportCopy:
      "Choose the Unity workspace path that HGP should inspect for versioning and build from this host directly.",
    unsupportedMessage: null,
  };
}

function resolveBuildTargetWizardAdapter(
  engineKind: RepositoryEngineKind,
  projectKind: ProjectDraft["projectKind"],
): BuildTargetWizardAdapter {
  const engineLabel = formatRepositoryEngineKindLabel(engineKind);
  const projectLabel = formatProjectKindLabel(projectKind).toLocaleLowerCase();

  if (engineKind === "unity") {
    return {
      kind: "unity",
      stepLabel: "Build Targets",
      stepDescription: `Configure the ${engineLabel}-specific build targets HGP should execute for this ${projectLabel}.`,
      supportTitle: "Unity target adapter",
      supportDescription:
        "This step is currently being driven by the Unity build target adapter.",
      supportCopy:
        "Unity projects define the target platform, build method, and editor executable that HGP should launch for each build target.",
      reviewDescription:
        "Engine-specific target configuration that HGP will execute for this project.",
      unsupportedMessage: null,
    };
  }

  return {
    kind: "engine-unsupported",
    stepLabel: "Build Targets",
    stepDescription:
      "Configure the engine-specific build targets HGP should execute for this project.",
    supportTitle: `${engineLabel} target adapter`,
    supportDescription:
      "This step must switch to the adapter owned by the selected engine.",
    supportCopy: `${engineLabel} projects need a specialized build target adapter before project creation can collect engine-specific fields.`,
    reviewDescription: `Engine-specific target configuration for ${engineLabel} is not available in project creation yet.`,
    unsupportedMessage: `${engineLabel} build target setup does not have a create-project adapter yet.`,
  };
}

function buildWizardSteps(
  sourceAdapter: ProjectSourceWizardAdapter,
  buildTargetAdapter: BuildTargetWizardAdapter,
): WizardStepDefinition[] {
  const definitions: Record<WizardStepKey, WizardStepDefinition> = {
    identity: {
      key: "identity",
      label: "Identity",
      description:
        "Name the project and choose the source and engine adapters HGP should use.",
    },
    access: {
      key: "access",
      label: sourceAdapter.stepLabel,
      description: sourceAdapter.stepDescription,
    },
    targets: {
      key: "targets",
      label: buildTargetAdapter.stepLabel,
      description: buildTargetAdapter.stepDescription,
    },
    publish: {
      key: "publish",
      label: "Publish Destinations",
      description:
        "Bind build outputs to publish destinations and validate destination-specific delivery rules before save.",
    },
    paths: {
      key: "paths",
      label: "Paths",
      description:
        "Choose optional artifact and workspace paths for this project.",
    },
    review: {
      key: "review",
      label: "Review",
      description:
        "Review the project definition produced by the selected source and engine adapters before registration.",
    },
  };

  return WIZARD_STEP_ORDER.map((stepKey) => definitions[stepKey]);
}

function createInitialProjectDraft(): ProjectDraft {
  return {
    projectKind: "repository",
    engineKind: "unity",
    name: "",
    repositoryUrl: "",
    localPath: "",
    repositoryVisibility: "public",
    pollingIntervalSeconds: "300",
    artifactsRootOverride: "",
    workspaceRootOverride: "",
    unityExecutablePath: "",
    buildTargets: [],
    publishDestinations: [],
  };
}

function createInitialCreateProjectWizardSnapshot(): CreateProjectWizardSnapshot {
  return {
    attemptedSteps: {
      ...EMPTY_VALIDATION_ATTEMPTS,
    },
    currentStepIndex: 0,
    draft: cloneProjectDraft(INITIAL_PROJECT_DRAFT),
    expandedTargetIds: {},
    unityExecutableDiagnostics: null,
    pendingBuildTargetRemovalId: null,
    repositoryCredentialId: null,
    touchedFields: {},
  };
}

function createEmptyBuildTargetDraft(index: number): BuildTargetDraft {
  return {
    id: `target-${index}`,
    name: "",
    targetPlatform: "",
    buildMethod: "",
  };
}

function toWizardBuildTargetDraft(
  target: SharedBuildTargetDraft,
): BuildTargetDraft {
  return {
    id: target.id,
    name: target.name,
    targetPlatform: target.targetPlatform,
    buildMethod: target.buildMethod,
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
    draft.projectKind === "repository" &&
    repositoryInventory.some(
      (repository) =>
        repository.repository_name.trim().toLocaleLowerCase() ===
        normalizedName.toLocaleLowerCase(),
    )
  ) {
    errors.name = "Another repository project already uses this name.";
  }

  if (draft.engineKind !== "unity") {
    errors.engineKind = `${formatRepositoryEngineKindLabel(draft.engineKind)} does not have a create-project build target adapter yet.`;
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
  sourceAdapter: ProjectSourceWizardAdapter,
) {
  const errors: {
    repositoryUrl?: string;
    localPath?: string;
    pollingIntervalSeconds?: string;
    repositoryAccess?: string;
  } = {};

  if (sourceAdapter.kind === "local") {
    const normalizedLocalPath = draft.localPath.trim();

    if (!normalizedLocalPath) {
      errors.localPath = "Local workspace path is required.";
    } else if (!looksLikeAbsolutePath(normalizedLocalPath)) {
      errors.localPath = "Local workspace path must be an absolute path.";
    } else if (
      repositoryInventory.some(
        (repository) =>
          normalizePathForComparison(repository.local_path ?? "") ===
          normalizePathForComparison(normalizedLocalPath),
      )
    ) {
      errors.localPath = "This local workspace is already registered in HGP.";
    }

    return errors;
  }

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

  if (draft.projectKind === "repository") {
    const pollingInterval = Number(draft.pollingIntervalSeconds.trim());
    if (!Number.isInteger(pollingInterval)) {
      errors.pollingIntervalSeconds =
        "Polling interval must be a whole number.";
    } else if (pollingInterval < 5) {
      errors.pollingIntervalSeconds =
        "Polling interval must be at least 5 seconds.";
    }
  }

  if (
    !errors.repositoryUrl &&
    (normalizedUrl.startsWith("https://") ||
      normalizedUrl.startsWith("http://"))
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
            : authState.repositoryAccessAssessment.provider_label ||
              "this host";
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
        } else if (authState.repositoryCredentialCount <= 0) {
          errors.repositoryAccess =
            "No stored repository credentials are available yet. Save one before setup can continue.";
        } else {
          errors.repositoryAccess = `Private ${providerLabel} repository detected. Select a stored repository credential before setup can continue.`;
        }
      }
    }
  }

  return errors;
}

function validateTargetsStep(
  draft: ProjectDraft,
  unityExecutableDiagnostics: UnityExecutableValidation | null,
  isValidatingUnityExecutable: boolean,
  buildTargetAdapter: BuildTargetWizardAdapter,
): TargetStepErrors {
  const errors: TargetStepErrors = {
    targets: {},
  };
  if (buildTargetAdapter.kind !== "unity") {
    errors.root =
      buildTargetAdapter.unsupportedMessage ||
      "The selected engine does not have a create-project build target adapter yet.";

    return errors;
  }

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

    errors.targets[target.id] = fieldErrors;
  }

  if (!draft.unityExecutablePath.trim()) {
    errors.root = "Unity executable path is required for all build targets.";
  } else if (isValidatingUnityExecutable) {
    errors.root = "Unity executable validation is still running.";
  } else if (!unityExecutableDiagnostics) {
    errors.root = "Unity executable path has not been validated yet.";
  } else if (unityExecutableDiagnostics.status !== "ready") {
    errors.root =
      unityExecutableDiagnostics.message || "Unity executable path is invalid.";
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
    errors.localPath ||
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
  return Boolean(errors.name || errors.targetPlatform || errors.buildMethod);
}

function firstBuildTargetFieldError(errors: TargetFieldErrors) {
  return errors.name || errors.targetPlatform || errors.buildMethod || null;
}

function formatBuildTargetExecutableSummary(
  diagnostics: UnityExecutableValidation | null,
  isValidating: boolean,
) {
  if (isValidating) {
    return "checking";
  }

  if (!diagnostics) {
    return "pending";
  }

  return formatDiagnosticStatus(diagnostics.status);
}

function buildBuildTargetQuickViewCopy(
  target: BuildTargetDraft,
  diagnostics: UnityExecutableValidation | null,
  unityExecutablePath: string,
) {
  if (diagnostics && diagnostics.status !== "ready") {
    return diagnostics.message;
  }

  if (!unityExecutablePath.trim()) {
    return "Unity executable path is still pending.";
  }

  return `${target.buildMethod.trim() || "Build method pending"} • ${unityExecutablePath.trim()}`;
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
  inventoryError: string | null;
}): WizardStepKey | null {
  if (
    input.isLoadingRepositoryInventory ||
    input.inventoryError ||
    hasIdentityErrors(input.identityErrors)
  ) {
    return "identity";
  }
  if (
    input.isLoadingRepositoryInventory ||
    input.inventoryError ||
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
  const isRepositoryProject = draft.projectKind === "repository";

  return {
    name: draft.name.trim(),
    engine_kind: draft.engineKind,
    source_mode: isRepositoryProject ? "managed_repository" : "local_workspace",
    repository_url: isRepositoryProject ? draft.repositoryUrl.trim() : null,
    local_path: isRepositoryProject ? null : draft.localPath.trim(),
    repository_access_assessment: isRepositoryProject
      ? repositoryAccessAssessment
      : null,
    repository_credentials_id: isRepositoryProject
      ? repositoryCredentialId
      : null,
    artifacts_root_override: optionalTrimmedString(draft.artifactsRootOverride),
    workspace_root_override: optionalTrimmedString(draft.workspaceRootOverride),
    polling_interval_seconds: isRepositoryProject
      ? Number(draft.pollingIntervalSeconds.trim())
      : 0,
    build_targets: draft.buildTargets.map((target) =>
      buildCreateProjectBuildTargetInput(
        draft.engineKind,
        target,
        draft.unityExecutablePath,
      ),
    ),
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

function buildCreateProjectBuildTargetInput(
  engineKind: RepositoryEngineKind,
  target: BuildTargetDraft,
  unityExecutablePath: string,
) {
  if (engineKind !== "unity") {
    throw new Error(
      `${formatRepositoryEngineKindLabel(engineKind)} does not have a create-project build target adapter yet.`,
    );
  }

  return {
    name: target.name.trim(),
    contract: {
      unity: {
        target_platform: normalizeUnityTargetPlatformValue(
          target.targetPlatform,
        ),
        build_method: target.buildMethod.trim(),
      },
    },
    unity_executable_path: unityExecutablePath.trim(),
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

function listSelectableUnityEditors(
  unityAdapterSettings: UnityAdapterSettings | null,
): DiscoveredUnityEditor[] {
  return (
    unityAdapterSettings?.capability_profile.discovered_editors.filter(
      (editor) => editor.executable_exists && editor.executable_is_file,
    ) ?? []
  );
}

function buildDetectedUnityEditorOptions(
  editors: DiscoveredUnityEditor[],
  isLoadingUnityAdapterSettings: boolean,
  unityAdapterSettingsError: string | null,
): SelectOption[] {
  if (isLoadingUnityAdapterSettings) {
    return [
      {
        label: "Scanning installed Unity editors...",
        value: "",
      },
    ];
  }

  if (unityAdapterSettingsError) {
    return [
      {
        label: "Unable to load installed Unity editors",
        value: "",
      },
    ];
  }

  if (editors.length === 0) {
    return [
      {
        label: "No installed Unity editors detected",
        value: "",
      },
    ];
  }

  return [
    {
      label: "Choose a detected Unity editor",
      title: "Choose a detected Unity editor",
      value: "",
    },
    ...editors.map((editor) => ({
      label: editor.version,
      title: editor.install_root_path,
      value: editor.executable_path,
    })),
  ];
}

function buildDetectedUnityEditorHint(
  unityAdapterSettingsError: string | null,
  editorCount: number,
) {
  if (unityAdapterSettingsError) {
    return `${unityAdapterSettingsError} Use the manual path field below to continue.`;
  }

  if (editorCount === 0) {
    return "Choose a detected editor when available, or keep using the manual executable path field below.";
  }

  return "Select a detected Unity install to fill the executable path below, or keep using the manual picker.";
}

function resolveDetectedUnityEditorValue(
  unityExecutablePath: string,
  editors: DiscoveredUnityEditor[],
) {
  const normalizedPath = unityExecutablePath.trim();

  return editors.some(
    (editor) => editor.executable_path.trim() === normalizedPath,
  )
    ? normalizedPath
    : "";
}

function cloneProjectDraft(draft: ProjectDraft): ProjectDraft {
  return {
    ...draft,
    buildTargets: draft.buildTargets.map((target) => ({
      ...target,
    })),
    publishDestinations: draft.publishDestinations.map((destination) => ({
      ...destination,
      bindings: destination.bindings.map((binding) => ({
        ...binding,
      })),
    })),
  };
}

function normalizeWizardStepIndex(stepIndex: number) {
  return Math.min(Math.max(stepIndex, 0), WIZARD_STEP_ORDER.length - 1);
}

function resolveNextBuildTargetIndex(buildTargets: BuildTargetDraft[]) {
  return buildTargets.reduce((nextIndex, target) => {
    const match = /^target-(\d+)$/.exec(target.id.trim());

    if (!match) {
      return nextIndex;
    }

    return Math.max(nextIndex, Number(match[1]) + 1);
  }, 1);
}

function buildProjectDraftDirtyKey(draft: ProjectDraft) {
  return JSON.stringify(cloneProjectDraft(draft));
}

function buildCreateProjectWizardDirtyState(
  draft: ProjectDraft,
  currentStepIndex: number,
  repositoryCredentialId: number | null,
) {
  if (currentStepIndex > 0 || repositoryCredentialId !== null) {
    return true;
  }

  return buildProjectDraftDirtyKey(draft) !== INITIAL_PROJECT_DRAFT_DIRTY_KEY;
}

function indexOfWizardStep(stepKey: WizardStepKey) {
  return WIZARD_STEP_ORDER.findIndex((step) => step === stepKey);
}

function formatProjectSourceReviewDescription(draft: ProjectDraft) {
  if (draft.projectKind === "repository") {
    return draft.repositoryUrl.trim() || "Repository source not set yet.";
  }

  return draft.localPath.trim() || "Local workspace source not set yet.";
}

function renderWizardAdapterUnavailableState(message: string) {
  return (
    <div className="wizard-callout wizard-callout--compact">
      <p className="wizard-callout__copy">{message}</p>
    </div>
  );
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

function normalizePathForComparison(value: string) {
  const normalized = value.trim().replace(/\\/g, "/");

  if (normalized === "/" || /^[a-zA-Z]:\/$/.test(normalized)) {
    return normalized.toLocaleLowerCase();
  }

  return normalized.replace(/\/+$/, "").toLocaleLowerCase();
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

  if (
    detection.supports_interactive_login &&
    detection.provider_id === "github"
  ) {
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

function isGithubAuthProviderResult(result: AuthProviderConnectionResult) {
  return (
    result.provider.provider_id === "github" ||
    result.provider.label.trim().toLocaleLowerCase() === "github" ||
    result.provider.instance_url
      .trim()
      .toLocaleLowerCase()
      .includes("github.com")
  );
}

function buildAuthProviderRoundTripMessage(
  result: AuthProviderConnectionResult,
  repositoryCredentialId: number | null,
  previousGithubCredentialId: number | null,
) {
  const nextCredentialId = result.provider.credential_id;
  const shouldSelectCredential =
    nextCredentialId !== null &&
    (repositoryCredentialId === null ||
      repositoryCredentialId === previousGithubCredentialId);

  if (shouldSelectCredential) {
    return `${result.message} The connected credential is now selected for this project draft.`;
  }

  return `${result.message} Return to the repository credential field to choose how this draft should bind the refreshed account.`;
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

function isRepositoryCredentialSelectable(credential: SecretCredentialSetting) {
  return (
    credential.config_summary.status === "ready" &&
    [
      "git-http-basic",
      "git-http-bearer",
      "git-http-github-host-login",
    ].includes(credential.kind)
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
