import {
  startTransition,
  type ReactNode,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";

import { Button, IconButton } from "../Button";
import {
  BuildTargetEditorOverlay,
  type BuildTargetEditorOverlayResult,
  type SharedBuildTargetDraft,
} from "../BuildTargetEditorOverlay";
import { BuildTargetRemovalCallout } from "../BuildTargetRemovalCallout";
import FullScreenModal from "../FullScreenModal";
import { type IconName } from "../Icon";
import {
  buildPublishDestinationDrafts,
  buildUpdateProjectPublishTargetsInput,
  collectBuildTargetBindingImpact,
  hasPublishDestinationValidationErrors,
  removeBuildTargetBindings,
  type ProjectBuildTargetReference,
  type PublishDestinationValidationErrors,
  validatePublishDestinationDrafts,
} from "../PublishDestinationsEditor";
import { type SelectOption } from "../Field";
import { FocusPageFrame, MetaItem, MetaRow, SummaryStrip } from "../Surface";
import { useOverlay } from "../OverlayManager";
import {
  useLocalization,
  type LocalizationVariables,
  type Translate,
} from "../../LocalizationProvider";
import {
  type BuildTargetDraft,
  type PathStepErrors,
  type ProjectDraft,
  type TargetFieldErrors,
  type TargetStepErrors,
  type WizardStepKey,
  buildDetectedUnityEditorHint,
  buildDetectedUnityEditorOptions,
  buildRepositoryAccessAssessmentFromDetection,
  buildWizardSteps,
  createInitialProjectDraft,
  formatProjectKindLabel,
  formatRepositoryEngineKindLabel,
  formatRepositoryAccessSummary,
  looksLikeAbsolutePath,
  normalizeUnityTargetPlatformValue,
  ProjectIdentityStep,
  ProjectLocalAccessStep,
  ProjectPathsStep,
  ProjectPublishStep,
  ProjectRepositoryAccessPanel,
  ProjectRepositoryAccessStep,
  ProjectTargetsStep,
  resolveBuildTargetWizardAdapter,
  resolveDetectedUnityEditorValue,
  resolveProjectSourceWizardAdapter,
  supportsShellRepositoryLoginAction,
} from "../wizard/ProjectDefinitionSteps";
import {
  connectRepositoryAuth,
  detectRepositoryProvider,
  disconnectRepositoryAuth,
  loadRepositoryInspection,
  loadRepositoryProjectDetail,
  loadSecretSettings,
  loadUnityAdapterSettings,
  removeRepositoryProject,
  reconnectRepositoryAuth,
  saveSecretCredential,
  updateRepositoryProject,
  validateUnityExecutablePath,
  type RemoveRepositoryProjectReport,
  type RemoveRepositoryProjectStrategy,
  type RepositoryAccessAssessment,
  type DiscoveredUnityEditor,
  type RepositoryEngineKind,
  type RepositoryInspectionEntry,
  type SaveSecretCredentialInput,
  type SecretCredentialSetting,
  type UpdateRepositoryProjectInput,
  type UnityExecutableValidation,
} from "../../services/projects";
import {
  loadAuthProviders,
  loginWithGithubAuth,
  type AuthProviderStatus,
} from "../../services/auth";
import {
  buildLocalizedProjectSourceDisplay,
  resolveLocalizedProjectSourceModeSummary,
} from "../../projectSourcePresentation";

type RepositoryProjectDetailProps = {
  onProjectNameResolved?: (repositoryName: string) => void;
  onProjectRemoved?: (report: RemoveRepositoryProjectReport) => void;
  repositoryId: number;
};

type ValidationState = {
  identity: {
    engineKind?: string;
    name?: string;
    projectKind?: string;
  };
  access: {
    localPath?: string;
    pollingIntervalSeconds?: string;
    repositoryAccess?: string;
    repositoryUrl?: string;
  };
  targets: TargetStepErrors;
  publish: PublishDestinationValidationErrors;
  paths: PathStepErrors;
};

type ProjectDetailTab = {
  description: string;
  icon: IconName;
  key: WizardStepKey;
  label: string;
};

type InitialUnityExecutableState = {
  hasMixedValues: boolean;
  sharedPath: string;
};

type ProjectRemovalOverlayProps = {
  hasPendingChanges: boolean;
  onResolve?: (value?: RemoveRepositoryProjectStrategy | null) => void;
  projectName: string;
};

const EMPTY_ATTEMPTED_STEPS: Record<WizardStepKey, boolean> = {
  identity: false,
  access: false,
  targets: false,
  publish: false,
  paths: false,
  review: false,
};

const DEFAULT_ACTIVE_STEP: WizardStepKey = "identity";
const MIN_PROJECT_POLL_INTERVAL_SECONDS = 5;

function translateMessage(
  t: Translate | undefined,
  key: string,
  fallbackText: string,
  variables?: LocalizationVariables,
) {
  return t ? t(key, fallbackText, variables) : fallbackText;
}

export function RepositoryProjectDetail({
  onProjectNameResolved,
  onProjectRemoved,
  repositoryId,
}: RepositoryProjectDetailProps) {
  const { openOverlay } = useOverlay();
  const { t } = useLocalization();
  const [repository, setRepository] =
    useState<RepositoryInspectionEntry | null>(null);
  const [repositoryInventory, setRepositoryInventory] = useState<
    RepositoryInspectionEntry[]
  >([]);
  const [draft, setDraft] = useState<ProjectDraft | null>(null);
  const [persistedDraftKey, setPersistedDraftKey] = useState<string | null>(
    null,
  );
  const [initialRepositoryCredentialId, setInitialRepositoryCredentialId] =
    useState<number | null>(null);
  const [repositoryCredentialId, setRepositoryCredentialId] = useState<
    number | null
  >(null);
  const [initialUnityExecutableState, setInitialUnityExecutableState] =
    useState<InitialUnityExecutableState>({
      hasMixedValues: false,
      sharedPath: "",
    });
  const [activeStepKey, setActiveStepKey] =
    useState<WizardStepKey>(DEFAULT_ACTIVE_STEP);
  const [attemptedSteps, setAttemptedSteps] = useState(() => ({
    ...EMPTY_ATTEMPTED_STEPS,
  }));
  const [touchedFields, setTouchedFields] = useState<Record<string, boolean>>(
    {},
  );
  const [validationState, setValidationState] = useState<ValidationState>(
    createEmptyValidationState,
  );
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [isRemovingProject, setIsRemovingProject] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saveMessage, setSaveMessage] = useState<string | null>(null);
  const [githubAuthProvider, setGithubAuthProvider] =
    useState<AuthProviderStatus | null>(null);
  const [isLoadingAuthProviders, setIsLoadingAuthProviders] = useState(false);
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
    useState(false);
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
  const [repositoryAccessActionMessage, setRepositoryAccessActionMessage] =
    useState<string | null>(null);
  const [pendingRepositoryAccessAction, setPendingRepositoryAccessAction] =
    useState(false);
  const [unityExecutableDiagnostics, setUnityExecutableDiagnostics] =
    useState<UnityExecutableValidation | null>(null);
  const [isValidatingUnityExecutable, setIsValidatingUnityExecutable] =
    useState(false);
  const [isLoadingUnityAdapterSettings, setIsLoadingUnityAdapterSettings] =
    useState(false);
  const [unityAdapterSettingsError, setUnityAdapterSettingsError] = useState<
    string | null
  >(null);
  const [detectedUnityEditors, setDetectedUnityEditors] = useState<
    DiscoveredUnityEditor[]
  >([]);
  const [pendingBuildTargetRemovalId, setPendingBuildTargetRemovalId] =
    useState<string | null>(null);
  const nextBuildTargetIdRef = useRef(1);
  const unityExecutableValidationTimerRef = useRef<number | undefined>(
    undefined,
  );
  const unityExecutableValidationTokenRef = useRef(0);
  const accessAssessmentTimerRef = useRef<number | undefined>(undefined);
  const accessAssessmentTokenRef = useRef(0);

  const loadRepositoryInventoryEffect = useEffectEvent(async () => {
    try {
      const inspection = await loadRepositoryInspection();
      startTransition(() => {
        setRepositoryInventory(inspection.repositories);
      });
    } catch {
      startTransition(() => {
        setRepositoryInventory([]);
      });
    }
  });

  const loadAuthProvidersEffect = useEffectEvent(async () => {
    setIsLoadingAuthProviders(true);

    try {
      const providers = await loadAuthProviders();
      const githubProvider = providers.find(
        (provider) => provider.provider_id === "github",
      );

      startTransition(() => {
        setGithubAuthProvider(githubProvider ?? null);
        setAuthProviderError(null);
        setIsLoadingAuthProviders(false);
      });
    } catch (loadError) {
      startTransition(() => {
        setGithubAuthProvider(null);
        setAuthProviderError(buildProjectSaveErrorMessage(loadError, t));
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
        setPublishCredentials(
          credentials.filter(
            (credential) => credential.kind === "itch-api-key",
          ),
        );
        setRepositoryCredentialsError(null);
        setIsLoadingRepositoryCredentials(false);
      });
    } catch (loadError) {
      startTransition(() => {
        setRepositoryCredentials([]);
        setPublishCredentials([]);
        setRepositoryCredentialsError(
          buildProjectSaveErrorMessage(loadError, t),
        );
        setIsLoadingRepositoryCredentials(false);
      });
    }
  });

  const loadUnityAdapterSettingsEffect = useEffectEvent(async () => {
    setIsLoadingUnityAdapterSettings(true);

    try {
      const settings = await loadUnityAdapterSettings();
      const editors =
        settings.capability_profile.discovered_editors.filter(
          (editor) => editor.executable_exists && editor.executable_is_file,
        ) ?? [];

      startTransition(() => {
        setDetectedUnityEditors(editors);
        setUnityAdapterSettingsError(null);
        setIsLoadingUnityAdapterSettings(false);
      });
    } catch (loadError) {
      startTransition(() => {
        setDetectedUnityEditors([]);
        setUnityAdapterSettingsError(
          buildProjectSaveErrorMessage(loadError, t),
        );
        setIsLoadingUnityAdapterSettings(false);
      });
    }
  });

  const resolveRepositoryDetail = useEffectEvent(async () => {
    return loadRepositoryProjectDetail(repositoryId);
  });

  const loadRepositoryDetail = useEffectEvent(async (showLoading = true) => {
    if (showLoading) {
      setIsLoading(true);
    }

    try {
      const matchingRepository = await resolveRepositoryDetail();

      if (!matchingRepository) {
        startTransition(() => {
          setRepository(null);
          setDraft(null);
          setPersistedDraftKey(null);
          setInitialRepositoryCredentialId(null);
          setRepositoryCredentialId(null);
          setError(null);
          setIsLoading(false);
        });
        return;
      }

      const nextDraft = buildRepositoryProjectDraft(matchingRepository);
      const nextDraftKey = buildProjectDraftDirtyKey(nextDraft);
      const nextUnityState = resolveInitialUnityExecutableState(
        nextDraft.buildTargets,
      );

      startTransition(() => {
        setRepository(matchingRepository);
        setDraft(nextDraft);
        setPersistedDraftKey(nextDraftKey);
        setInitialUnityExecutableState(nextUnityState);
        setValidationState(createEmptyValidationState());
        setTouchedFields({});
        setAttemptedSteps({
          ...EMPTY_ATTEMPTED_STEPS,
        });
        setActiveStepKey(DEFAULT_ACTIVE_STEP);
        setRepositoryAccessAssessment(
          buildRepositoryAccessAssessmentFromRepository(matchingRepository),
        );
        setRepositoryAccessError(null);
        setIsAssessingRepositoryAccess(false);
        setInitialRepositoryCredentialId(
          matchingRepository.credentials?.credential_id ?? null,
        );
        setRepositoryCredentialId(
          matchingRepository.credentials?.credential_id ?? null,
        );
        setRepositoryAccessActionMessage(null);
        setPendingRepositoryAccessAction(false);
        setUnityExecutableDiagnostics(
          resolveInitialUnityExecutableDiagnostics(nextDraft.buildTargets),
        );
        setIsValidatingUnityExecutable(false);
        setPendingBuildTargetRemovalId(null);
        setError(null);
        setIsLoading(false);
        if (showLoading) {
          setSaveError(null);
          setSaveMessage(null);
        }
      });

      nextBuildTargetIdRef.current = resolveNextBuildTargetIndex(
        nextDraft.buildTargets,
      );
    } catch (loadError) {
      startTransition(() => {
        setError(buildProjectDetailErrorMessage(loadError, t));
        setIsLoading(false);
      });
    }
  });

  useEffect(() => {
    startTransition(() => {
      setRepository(null);
      setDraft(null);
      setPersistedDraftKey(null);
      setInitialRepositoryCredentialId(null);
      setRepositoryCredentialId(null);
      setValidationState(createEmptyValidationState());
      setTouchedFields({});
      setAttemptedSteps({
        ...EMPTY_ATTEMPTED_STEPS,
      });
      setActiveStepKey(DEFAULT_ACTIVE_STEP);
      setSaveError(null);
      setSaveMessage(null);
      setPendingBuildTargetRemovalId(null);
      setRepositoryAccessAssessment(null);
      setRepositoryAccessError(null);
      setRepositoryAccessActionMessage(null);
      setPendingRepositoryAccessAction(false);
      setGithubAuthProvider(null);
      setAuthProviderError(null);
      setRepositoryCredentials([]);
      setPublishCredentials([]);
      setRepositoryCredentialsError(null);
      setUnityExecutableDiagnostics(null);
      setIsValidatingUnityExecutable(false);
      setDetectedUnityEditors([]);
      setUnityAdapterSettingsError(null);
      setInitialUnityExecutableState({
        hasMixedValues: false,
        sharedPath: "",
      });
    });

    void loadRepositoryInventoryEffect();
    void loadAuthProvidersEffect();
    void loadRepositoryCredentialsEffect();
    void loadUnityAdapterSettingsEffect();
    void loadRepositoryDetail(true);

    return () => {
      if (unityExecutableValidationTimerRef.current !== undefined) {
        window.clearTimeout(unityExecutableValidationTimerRef.current);
      }

      if (accessAssessmentTimerRef.current !== undefined) {
        window.clearTimeout(accessAssessmentTimerRef.current);
      }
    };
  }, [repositoryId]);

  useEffect(() => {
    const resolvedName =
      draft?.name.trim() || repository?.repository_name.trim();

    if (!resolvedName) {
      return;
    }

    onProjectNameResolved?.(resolvedName);
  }, [draft?.name, onProjectNameResolved, repository?.repository_name]);

  const requestRepositoryAccessAssessment = useEffectEvent(
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
          t,
        );

        startTransition(() => {
          setRepositoryAccessAssessment(assessment);
          setRepositoryAccessError(null);
          setIsAssessingRepositoryAccess(false);
        });
      } catch (assessmentError) {
        if (accessAssessmentTokenRef.current !== assessmentToken) {
          return;
        }

        startTransition(() => {
          setRepositoryAccessAssessment(null);
          setRepositoryAccessError(
            buildProjectSaveErrorMessage(assessmentError, t),
          );
          setIsAssessingRepositoryAccess(false);
        });
      }
    },
  );

  useEffect(() => {
    if (!draft || draft.projectKind !== "repository") {
      if (accessAssessmentTimerRef.current !== undefined) {
        window.clearTimeout(accessAssessmentTimerRef.current);
        accessAssessmentTimerRef.current = undefined;
      }

      accessAssessmentTokenRef.current += 1;
      startTransition(() => {
        setRepositoryAccessAssessment(null);
        setRepositoryAccessError(null);
        setIsAssessingRepositoryAccess(false);
        setRepositoryAccessActionMessage(null);
      });
      return;
    }

    const repositoryUrl = draft.repositoryUrl.trim();
    const persistedAssessment = repository
      ? buildRepositoryAccessAssessmentFromRepository(repository)
      : null;

    if (
      repository &&
      persistedAssessment &&
      repositoryUrl === repository.repo_url.trim() &&
      draft.repositoryVisibility ===
        resolveRepositoryVisibilitySelection(repository)
    ) {
      if (accessAssessmentTimerRef.current !== undefined) {
        window.clearTimeout(accessAssessmentTimerRef.current);
        accessAssessmentTimerRef.current = undefined;
      }

      accessAssessmentTokenRef.current += 1;
      startTransition(() => {
        setRepositoryAccessAssessment(persistedAssessment);
        setRepositoryAccessError(null);
        setIsAssessingRepositoryAccess(false);
        setRepositoryAccessActionMessage(null);
      });
      return;
    }

    if (
      !repositoryUrl ||
      !(
        repositoryUrl.startsWith("https://") ||
        repositoryUrl.startsWith("http://")
      )
    ) {
      if (accessAssessmentTimerRef.current !== undefined) {
        window.clearTimeout(accessAssessmentTimerRef.current);
        accessAssessmentTimerRef.current = undefined;
      }

      accessAssessmentTokenRef.current += 1;
      startTransition(() => {
        setRepositoryAccessAssessment(null);
        setRepositoryAccessError(null);
        setIsAssessingRepositoryAccess(false);
        setRepositoryAccessActionMessage(null);
      });
      return;
    }

    if (accessAssessmentTimerRef.current !== undefined) {
      window.clearTimeout(accessAssessmentTimerRef.current);
    }

    accessAssessmentTokenRef.current += 1;
    const assessmentToken = accessAssessmentTokenRef.current;

    startTransition(() => {
      setIsAssessingRepositoryAccess(true);
      setRepositoryAccessError(null);
      setRepositoryAccessActionMessage(null);
    });

    accessAssessmentTimerRef.current = window.setTimeout(() => {
      void requestRepositoryAccessAssessment(
        repositoryUrl,
        draft.repositoryVisibility,
        assessmentToken,
      );
    }, 250);

    return () => {
      if (accessAssessmentTimerRef.current !== undefined) {
        window.clearTimeout(accessAssessmentTimerRef.current);
        accessAssessmentTimerRef.current = undefined;
      }
    };
  }, [
    repository,
    draft?.projectKind,
    draft?.repositoryUrl,
    draft?.repositoryVisibility,
    requestRepositoryAccessAssessment,
  ]);

  const markFieldTouched = useEffectEvent((fieldName: string) => {
    startTransition(() => {
      setTouchedFields((current) => ({
        ...current,
        [fieldName]: true,
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
          } catch (validationError) {
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
                message: buildProjectSaveErrorMessage(validationError, t),
              });
              setIsValidatingUnityExecutable(false);
            });
          }
        },
        delayMillis,
      );
    },
  );

  const handleDraftFieldChange = useEffectEvent(
    <FieldName extends keyof ProjectDraft>(
      fieldName: FieldName,
      value: ProjectDraft[FieldName],
    ) => {
      startTransition(() => {
        setDraft((currentDraft) => {
          if (!currentDraft) {
            return currentDraft;
          }

          return {
            ...currentDraft,
            [fieldName]: value,
          };
        });
      });
    },
  );

  const handleProjectPathCleared = useEffectEvent(
    (
      fieldName:
        | "localPath"
        | "artifactsRootOverride"
        | "workspaceRootOverride",
    ) => {
      handleDraftFieldChange(fieldName, "");
      markFieldTouched(fieldName);
    },
  );

  const handleProjectPathPicked = useEffectEvent(
    (
      fieldName:
        | "localPath"
        | "artifactsRootOverride"
        | "workspaceRootOverride",
      selectedPath: string,
    ) => {
      handleDraftFieldChange(fieldName, selectedPath);
      markFieldTouched(fieldName);
    },
  );

  const handlePathPickerError = useEffectEvent((pickError: unknown) => {
    startTransition(() => {
      setSaveError(buildProjectSaveErrorMessage(pickError, t));
    });
  });

  const handleAddBuildTarget = useEffectEvent(async () => {
    if (!draft) {
      return;
    }

    const nextTarget = createEmptyBuildTargetDraft(
      nextBuildTargetIdRef.current,
    );
    const created = await openOverlay<BuildTargetEditorOverlayResult>(
      BuildTargetEditorOverlay,
      {
        existingTargets: draft.buildTargets.map(({ id, targetPlatform }) => ({
          id,
          targetPlatform,
        })),
        initialErrors: {},
        initialTarget: nextTarget,
        mode: "create",
        targetId: nextTarget.id,
      },
    );

    if (!created) {
      return;
    }

    nextBuildTargetIdRef.current += 1;

    startTransition(() => {
      setDraft((currentDraft) => {
        if (!currentDraft) {
          return currentDraft;
        }

        return {
          ...currentDraft,
          buildTargets: [
            ...currentDraft.buildTargets,
            {
              ...toProjectBuildTargetDraft(created.target),
              buildTargetId: null,
              unityExecutablePath: currentDraft.unityExecutablePath,
            },
          ],
        };
      });
      setActiveStepKey("targets");
    });
  });

  const handleEditBuildTarget = useEffectEvent(async (targetId: string) => {
    if (!draft) {
      return;
    }

    const target = draft.buildTargets.find((entry) => entry.id === targetId);
    if (!target) {
      return;
    }

    const updated = await openOverlay<BuildTargetEditorOverlayResult>(
      BuildTargetEditorOverlay,
      {
        existingTargets: draft.buildTargets.map(({ id, targetPlatform }) => ({
          id,
          targetPlatform,
        })),
        initialErrors: validationState.targets.targets[targetId] ?? {},
        initialTarget: target,
        mode: "edit",
        targetId,
      },
    );

    if (!updated) {
      return;
    }

    startTransition(() => {
      setDraft((currentDraft) => {
        if (!currentDraft) {
          return currentDraft;
        }

        return {
          ...currentDraft,
          buildTargets: currentDraft.buildTargets.map((entry) =>
            entry.id === targetId
              ? {
                  ...toProjectBuildTargetDraft(updated.target),
                  buildTargetId: entry.buildTargetId ?? null,
                  unityExecutablePath: entry.unityExecutablePath,
                }
              : entry,
          ),
        };
      });
    });
  });

  const handleRemoveBuildTarget = useEffectEvent((targetId: string) => {
    startTransition(() => {
      setPendingBuildTargetRemovalId(targetId);
    });
  });

  const handleConfirmBuildTargetRemoval = useEffectEvent(() => {
    if (!pendingBuildTargetRemovalId) {
      return;
    }

    startTransition(() => {
      setDraft((currentDraft) => {
        if (!currentDraft) {
          return currentDraft;
        }

        return {
          ...currentDraft,
          buildTargets: currentDraft.buildTargets.filter(
            (target) => target.id !== pendingBuildTargetRemovalId,
          ),
          publishDestinations: removeBuildTargetBindings(
            currentDraft.publishDestinations,
            pendingBuildTargetRemovalId,
          ),
        };
      });
      setPendingBuildTargetRemovalId(null);
    });
  });

  const handleRetryRepositoryAccessCheck = useEffectEvent(() => {
    if (!draft || draft.projectKind !== "repository") {
      return;
    }

    accessAssessmentTokenRef.current += 1;
    const assessmentToken = accessAssessmentTokenRef.current;

    startTransition(() => {
      setIsAssessingRepositoryAccess(true);
      setRepositoryAccessError(null);
    });

    void requestRepositoryAccessAssessment(
      draft.repositoryUrl.trim(),
      draft.repositoryVisibility,
      assessmentToken,
    );
  });

  const handleRetryAuthProviders = useEffectEvent(() => {
    void loadAuthProvidersEffect();
  });

  const handleRetryRepositoryCredentials = useEffectEvent(() => {
    void loadRepositoryCredentialsEffect();
  });

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
      setSaveError(null);
    });

    try {
      let provider = githubAuthProvider;
      if (provider?.status !== "connected" || !provider.credential_id) {
        provider = await loginWithGithubAuth({ force: false });
      }

      if (!provider.credential_id) {
        throw new Error(
          t(
            "project_detail.repository_access.error.missing_credential_id",
            "GitHub login completed without a reusable credential id.",
          ),
        );
      }

      startTransition(() => {
        setGithubAuthProvider(provider);
        setRepositoryCredentialId(provider.credential_id);
        setRepositoryAccessActionMessage(
          t(
            "project_detail.repository_access.message.github_connected",
            "GitHub login connected for this project. Save changes to keep the connection.",
          ),
        );
      });
    } catch (bindingError) {
      startTransition(() => {
        setSaveError(buildProjectSaveErrorMessage(bindingError, t));
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
        t(
          "project_detail.repository_access.message.cleared",
          "Repository credential cleared from the draft. Save changes to keep it disconnected.",
        ),
      );
      setSaveError(null);
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
            ? t(
                "project_detail.repository_access.message.selected",
                "Stored repository credential selected for this project. Save changes to keep the connection.",
              )
            : t(
                "project_detail.repository_access.message.cleared",
                "Repository credential cleared from the draft. Save changes to keep it disconnected.",
              ),
        );
        setSaveError(null);
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
          setDraft((currentDraft) => {
            if (!currentDraft) {
              return currentDraft;
            }

            return {
              ...currentDraft,
              publishDestinations: currentDraft.publishDestinations.map(
                (destination) =>
                  destination.id === destinationId
                    ? {
                        ...destination,
                        credentialsId: createdCredentialId,
                        credentialsName: credentialName,
                      }
                    : destination,
              ),
            };
          });
        });

        return createdCredentialId;
      } catch (saveCredentialError) {
        throw new Error(buildProjectSaveErrorMessage(saveCredentialError, t));
      }
    },
  );

  const handleSaveProject = useEffectEvent(async () => {
    if (!repository || !draft || isSaving || isRemovingProject) {
      return;
    }

    const nextValidationState = buildValidationState({
      draft,
      repositoryAccessAssessment,
      isAssessingRepositoryAccess,
      repositoryAccessError,
      repositoryCredentialId: resolveRepositoryCredentialIdForSave(
        draft.projectKind === "repository" ? repositoryAccessAssessment : null,
        draft.projectKind === "repository" ? repositoryCredentialId : null,
      ),
      githubAuthProvider,
      isLoadingAuthProviders,
      authProviderError,
      isLoadingRepositoryCredentials,
      repositoryCredentialsError,
      repositoryCredentialCount: repositoryCredentials.length,
      unityExecutableDiagnostics,
      isValidatingUnityExecutable,
      inventory: repositoryInventory.filter(
        (entry) => entry.repository_id !== repository.repository_id,
      ),
      t,
    });

    const invalidStep = findFirstInvalidStep(nextValidationState);
    if (invalidStep) {
      startTransition(() => {
        setValidationState(nextValidationState);
        setAttemptedSteps(
          buildAttemptedStepsForValidation(nextValidationState),
        );
        setActiveStepKey(invalidStep);
        setSaveError(null);
        setSaveMessage(null);
      });
      return;
    }

    setIsSaving(true);
    setSaveError(null);
    setSaveMessage(null);

    const desiredRepositoryCredentialId = resolveRepositoryCredentialIdForSave(
      draft.projectKind === "repository" ? repositoryAccessAssessment : null,
      draft.projectKind === "repository" ? repositoryCredentialId : null,
    );

    try {
      await updateRepositoryProject(
        buildRepositoryProjectUpdateInput(
          repository,
          draft,
          draft.projectKind === "repository"
            ? repositoryAccessAssessment
            : null,
          initialUnityExecutableState,
        ),
      );

      if (draft.projectKind === "repository") {
        const persistedRepositoryCredentialId =
          repository.credentials?.credential_id ?? null;

        if (desiredRepositoryCredentialId === null) {
          await disconnectCredentialIfBound(
            repository.repository_id,
            persistedRepositoryCredentialId,
          );
        } else if (persistedRepositoryCredentialId === null) {
          await connectRepositoryAuth(
            repository.repository_id,
            desiredRepositoryCredentialId,
          );
        } else {
          await reconnectRepositoryAuth(
            repository.repository_id,
            desiredRepositoryCredentialId,
          );
        }
      }

      await loadRepositoryDetail(false);

      startTransition(() => {
        setSaveMessage(
          t(
            "project_detail.save.success",
            "Saved changes for {{projectName}}.",
            { projectName: repository.repository_name },
          ),
        );
      });
    } catch (saveProjectError) {
      startTransition(() => {
        setSaveError(buildProjectSaveErrorMessage(saveProjectError, t));
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

  const handleOpenProjectRemoval = useEffectEvent(async () => {
    if (!repository || isSaving || isRemovingProject) {
      return;
    }

    const strategy = await openOverlay<RemoveRepositoryProjectStrategy | null>(
      ProjectRemovalOverlay,
      {
        hasPendingChanges: Boolean(hasPendingChangesRef.current),
        projectName: draft?.name.trim() || repository.repository_name,
      },
    );

    if (!strategy) {
      return;
    }

    setIsRemovingProject(true);
    setSaveError(null);

    try {
      const report = await removeRepositoryProject({
        repository_id: repository.repository_id,
        strategy,
      });

      onProjectRemoved?.(report);
    } catch (removeProjectError) {
      startTransition(() => {
        setSaveError(buildProjectRemoveErrorMessage(removeProjectError, t));
      });
    } finally {
      startTransition(() => {
        setIsRemovingProject(false);
      });
    }
  });

  const draftState = draft ?? createInitialProjectDraft();
  const projectSourceAdapter = resolveProjectSourceWizardAdapter(
    draftState.projectKind,
    t,
  );
  const buildTargetAdapter = resolveBuildTargetWizardAdapter(
    draftState.engineKind,
    draftState.projectKind,
    t,
  );
  const wizardSteps = buildWizardSteps(
    projectSourceAdapter,
    buildTargetAdapter,
    t,
  );
  const tabs = buildProjectDetailTabs(wizardSteps);
  const buildTargetReferences: ProjectBuildTargetReference[] =
    draftState.buildTargets.map((target) => ({
      id: target.id,
      buildTargetId: target.buildTargetId ?? null,
      name:
        target.name.trim() ||
        t("project_shared.target.unnamed", "Unnamed target"),
    }));
  const hasPendingChanges =
    draft !== null &&
    persistedDraftKey !== null &&
    (buildProjectDraftDirtyKey(draft) !== persistedDraftKey ||
      resolveRepositoryCredentialIdForSave(
        draft.projectKind === "repository" ? repositoryAccessAssessment : null,
        draft.projectKind === "repository" ? repositoryCredentialId : null,
      ) !== initialRepositoryCredentialId);
  const projectSourceInput = draft
    ? buildProjectSourcePresentationInput(draft)
    : buildProjectSourcePresentationInput(createInitialProjectDraft());
  const projectSourceDisplay = buildLocalizedProjectSourceDisplay(
    t,
    projectSourceInput,
  );
  const repositoryAccessSummary = draft
    ? formatRepositoryAccessSummary(
        draft.repositoryUrl,
        repositoryAccessAssessment,
        isAssessingRepositoryAccess,
        repositoryAccessError,
        t,
      )
    : t("project_shared.repository_access.summary.pending", "Pending");
  const pendingBuildTargetRemoval =
    pendingBuildTargetRemovalId && draft
      ? (draft.buildTargets.find(
          (target) => target.id === pendingBuildTargetRemovalId,
        ) ?? null)
      : null;
  const pendingBuildTargetBindingImpact =
    pendingBuildTargetRemoval && draft
      ? collectBuildTargetBindingImpact(
          draft.publishDestinations,
          pendingBuildTargetRemoval.id,
        )
      : [];
  const isEditingLocked = repository
    ? hasActiveRepositoryProcesses(repository)
    : false;
  const hasPendingChangesRef = useRef(hasPendingChanges);
  hasPendingChangesRef.current = hasPendingChanges;

  if (isLoading) {
    return (
      <div className="project-detail-shell">
        <FocusPageFrame
          description={t(
            "project_detail.frame.description",
            "Inspect and edit project definition steps with the same shared surfaces used by creation.",
          )}
          eyebrow={t("project_detail.frame.eyebrow", "Project")}
          title={t("project_detail.frame.title", "Project Detail")}
        >
          <div className="feed-state">
            <p className="feed-state__title">
              {t("project_detail.loading.title", "Loading project detail...")}
            </p>
            <p className="feed-state__copy">
              {t(
                "project_detail.loading.copy",
                "The shell is resolving the persisted project definition.",
              )}
            </p>
          </div>
        </FocusPageFrame>
      </div>
    );
  }

  if (error) {
    return (
      <div className="project-detail-shell">
        <FocusPageFrame
          description={t(
            "project_detail.frame.description",
            "Inspect and edit project definition steps with the same shared surfaces used by creation.",
          )}
          eyebrow={t("project_detail.frame.eyebrow", "Project")}
          title={t("project_detail.frame.title", "Project Detail")}
        >
          <div className="feed-state">
            <p className="feed-state__title">
              {t(
                "project_detail.error.title",
                "Project detail is unavailable.",
              )}
            </p>
            <p className="feed-state__copy">{error}</p>
            <Button
              leadingIcon="refresh"
              onClick={handleReloadProject}
              size="sm"
              variant="secondary"
            >
              {t("project_detail.actions.retry_load", "Retry project load")}
            </Button>
          </div>
        </FocusPageFrame>
      </div>
    );
  }

  if (!repository || !draft) {
    return (
      <div className="project-detail-shell">
        <FocusPageFrame
          description={t(
            "project_detail.frame.description",
            "Inspect and edit project definition steps with the same shared surfaces used by creation.",
          )}
          eyebrow={t("project_detail.frame.eyebrow", "Project")}
          title={t("project_detail.frame.title", "Project Detail")}
        >
          <div className="feed-state">
            <p className="feed-state__title">
              {t("project_detail.empty.title", "Project not found.")}
            </p>
            <p className="feed-state__copy">
              {t(
                "project_detail.empty.copy",
                "Reload the project detail to rebuild the editable project draft.",
              )}
            </p>
          </div>
        </FocusPageFrame>
      </div>
    );
  }

  return (
    <div className="project-detail-shell">
      <FocusPageFrame
        actions={
          <div className="project-detail-toolbar">
            <Button
              className="project-detail-toolbar__remove"
              disabled={isSaving || isRemovingProject || isEditingLocked}
              leadingIcon="trash"
              onClick={handleOpenProjectRemoval}
              size="sm"
              variant="secondary"
            >
              {t("project_detail.actions.remove", "Remove Project")}
            </Button>
            <Button
              disabled={isSaving || isRemovingProject}
              leadingIcon="refresh"
              onClick={handleReloadProject}
              size="sm"
              variant="secondary"
            >
              {t("project_detail.actions.reload", "Reload")}
            </Button>
            <Button
              disabled={
                !hasPendingChanges ||
                isSaving ||
                isRemovingProject ||
                isEditingLocked
              }
              onClick={() => void handleSaveProject()}
              size="sm"
              variant="primary"
            >
              {isSaving
                ? t("project_detail.actions.saving", "Saving...")
                : t("project_detail.actions.save", "Save Changes")}
            </Button>
          </div>
        }
        description={projectSourceDisplay}
        eyebrow={formatProjectKindLabel(draft.projectKind, t)}
        summary={
          <MetaRow>
            <MetaItem label={t("project_detail.summary.draft", "Draft")}>
              {hasPendingChanges
                ? t("project_detail.summary.unsaved", "Unsaved changes")
                : t("project_detail.summary.saved", "Saved")}
            </MetaItem>
            <MetaItem label={t("project_detail.summary.mode", "Mode")}>
              {resolveLocalizedProjectSourceModeSummary(t, projectSourceInput)}
            </MetaItem>
            <MetaItem label={t("project_detail.summary.targets", "Targets")}>
              {draft.buildTargets.length}
            </MetaItem>
            <MetaItem
              label={
                draft.projectKind === "repository"
                  ? t("project_detail.summary.access", "Access")
                  : t("project_detail.summary.source", "Source")
              }
            >
              {draft.projectKind === "repository"
                ? repositoryAccessSummary
                : t(
                    "projects.presentation.mode.direct_workspace",
                    "Direct workspace",
                  )}
            </MetaItem>
          </MetaRow>
        }
        title={draft.name.trim() || repository.repository_name}
      >
        {saveMessage ? <p className="notice-banner">{saveMessage}</p> : null}
        {saveError ? (
          <p className="feed-banner feed-banner--error">{saveError}</p>
        ) : null}
        {isEditingLocked ? (
          <p className="feed-banner">
            {t(
              "project_detail.editing_locked",
              "Project changes are available only when no related processes are running.",
            )}
          </p>
        ) : null}

        <div className="project-detail-stage-shell">
          <ProjectDetailSectionTabs
            activeSection={activeStepKey}
            disabled={isEditingLocked}
            onChange={setActiveStepKey}
            tabs={tabs}
          />

          <fieldset
            className="project-detail-edit-lock-shell"
            disabled={isEditingLocked}
          >
            <div className="project-detail-stage-shell__content">
              {tabs.map((tab) => (
                <ProjectDetailStepPanel
                  description={tab.description}
                  key={tab.key}
                  open={tab.key === activeStepKey}
                  sectionKey={tab.key}
                  summary={buildStepSummary(
                    tab.key,
                    draft,
                    repositoryAccessSummary,
                    t,
                  )}
                  title={tab.label}
                >
                  {tab.key === "identity" ? (
                    <ProjectIdentityStep
                      draft={draft}
                      errors={{
                        engineKind: shouldShowFieldError(
                          attemptedSteps.identity,
                          touchedFields,
                          "engineKind",
                        )
                          ? validationState.identity.engineKind
                          : undefined,
                        name: shouldShowFieldError(
                          attemptedSteps.identity,
                          touchedFields,
                          "name",
                        )
                          ? validationState.identity.name
                          : undefined,
                        projectKind: shouldShowFieldError(
                          attemptedSteps.identity,
                          touchedFields,
                          "projectKind",
                        )
                          ? validationState.identity.projectKind
                          : undefined,
                      }}
                      onEngineKindChange={(engineKind) => {
                        handleDraftFieldChange("engineKind", engineKind);
                        markFieldTouched("engineKind");
                      }}
                      onFieldBlur={markFieldTouched}
                      onNameChange={(value) => {
                        handleDraftFieldChange("name", value);
                        markFieldTouched("name");
                      }}
                      onProjectKindChange={(projectKind) => {
                        handleDraftFieldChange("projectKind", projectKind);
                        markFieldTouched("projectKind");
                      }}
                    />
                  ) : null}

                  {tab.key === "access" ? (
                    draft.projectKind === "repository" ? (
                      <ProjectRepositoryAccessStep
                        onPollingIntervalSecondsBlur={() =>
                          markFieldTouched("pollingIntervalSeconds")
                        }
                        onPollingIntervalSecondsChange={(value) => {
                          handleDraftFieldChange(
                            "pollingIntervalSeconds",
                            value,
                          );
                          markFieldTouched("pollingIntervalSeconds");
                        }}
                        onRepositoryUrlBlur={() => {
                          markFieldTouched("repositoryUrl");
                          markFieldTouched("repositoryAccess");
                        }}
                        onRepositoryUrlChange={(value) => {
                          handleDraftFieldChange("repositoryUrl", value);
                          markFieldTouched("repositoryUrl");
                          markFieldTouched("repositoryAccess");
                        }}
                        onRepositoryVisibilityBlur={() =>
                          markFieldTouched("repositoryAccess")
                        }
                        onRepositoryVisibilityChange={(value) => {
                          handleDraftFieldChange("repositoryVisibility", value);
                          markFieldTouched("repositoryAccess");
                        }}
                        pollingIntervalSeconds={draft.pollingIntervalSeconds}
                        pollingIntervalSecondsError={
                          shouldShowFieldError(
                            attemptedSteps.access,
                            touchedFields,
                            "pollingIntervalSeconds",
                          )
                            ? validationState.access.pollingIntervalSeconds
                            : undefined
                        }
                        repositoryAccessPanel={
                          <ProjectRepositoryAccessPanel
                            authProviderError={authProviderError}
                            githubAuthProvider={githubAuthProvider}
                            isAssessingRepositoryAccess={
                              isAssessingRepositoryAccess
                            }
                            isLoadingAuthProviders={isLoadingAuthProviders}
                            isLoadingRepositoryCredentials={
                              isLoadingRepositoryCredentials
                            }
                            onBindRepositoryAccess={() => {
                              void handleBindRepositoryAccess();
                            }}
                            onClearRepositoryAccessBinding={
                              handleClearRepositoryAccessBinding
                            }
                            onRepositoryCredentialChange={
                              handleRepositoryCredentialSelectionChange
                            }
                            onRetryAuthProviders={handleRetryAuthProviders}
                            onRetryRepositoryAccessCheck={
                              handleRetryRepositoryAccessCheck
                            }
                            onRetryRepositoryCredentials={
                              handleRetryRepositoryCredentials
                            }
                            pendingRepositoryAccessAction={
                              pendingRepositoryAccessAction
                            }
                            repositoryAccessActionMessage={
                              repositoryAccessActionMessage
                            }
                            repositoryAccessAssessment={
                              repositoryAccessAssessment
                            }
                            repositoryAccessError={repositoryAccessError}
                            repositoryCredentialId={repositoryCredentialId}
                            repositoryCredentialOptions={buildRepositoryCredentialOptions(
                              repositoryCredentials,
                              repositoryCredentialId,
                              isLoadingRepositoryCredentials,
                              t,
                            )}
                            repositoryCredentialsError={
                              repositoryCredentialsError
                            }
                            repositoryUrl={draft.repositoryUrl}
                            validationError={
                              shouldShowFieldError(
                                attemptedSteps.access,
                                touchedFields,
                                "repositoryAccess",
                              )
                                ? validationState.access.repositoryAccess
                                : undefined
                            }
                          />
                        }
                        repositoryUrl={draft.repositoryUrl}
                        repositoryUrlError={
                          shouldShowFieldError(
                            attemptedSteps.access,
                            touchedFields,
                            "repositoryUrl",
                          )
                            ? validationState.access.repositoryUrl
                            : undefined
                        }
                        repositoryVisibility={draft.repositoryVisibility}
                      />
                    ) : (
                      <ProjectLocalAccessStep
                        localPath={draft.localPath}
                        localPathError={
                          shouldShowFieldError(
                            attemptedSteps.access,
                            touchedFields,
                            "localPath",
                          )
                            ? validationState.access.localPath
                            : undefined
                        }
                        onClearLocalPath={() =>
                          handleProjectPathCleared("localPath")
                        }
                        onPathPickError={handlePathPickerError}
                        onPathPicked={(selectedPath) =>
                          handleProjectPathPicked("localPath", selectedPath)
                        }
                      />
                    )
                  ) : null}

                  {tab.key === "targets" ? (
                    <ProjectTargetsStep
                      buildTargetAdapter={buildTargetAdapter}
                      buildTargets={draft.buildTargets}
                      detectedEditorDisabled={
                        isLoadingUnityAdapterSettings ||
                        detectedUnityEditors.length === 0
                      }
                      detectedEditorHint={buildDetectedUnityEditorHint(
                        unityAdapterSettingsError,
                        detectedUnityEditors.length,
                        t,
                      )}
                      detectedEditorOptions={buildDetectedUnityEditorOptions(
                        detectedUnityEditors,
                        isLoadingUnityAdapterSettings,
                        unityAdapterSettingsError,
                        t,
                      )}
                      detectedEditorValue={resolveDetectedUnityEditorValue(
                        draft.unityExecutablePath,
                        detectedUnityEditors,
                      )}
                      isBusy={isSaving}
                      isValidatingUnityExecutable={isValidatingUnityExecutable}
                      onAddTarget={() => {
                        void handleAddBuildTarget();
                      }}
                      onDetectedEditorChange={(selectedPath) => {
                        handleDraftFieldChange(
                          "unityExecutablePath",
                          selectedPath,
                        );
                        setUnityExecutableDiagnostics(null);
                        scheduleUnityExecutableValidation(selectedPath);
                      }}
                      onEditTarget={(targetId) => {
                        void handleEditBuildTarget(targetId);
                      }}
                      onRemoveTarget={handleRemoveBuildTarget}
                      onUnityExecutablePickError={handlePathPickerError}
                      onUnityExecutablePicked={(selectedPath) => {
                        handleDraftFieldChange(
                          "unityExecutablePath",
                          selectedPath,
                        );
                        setUnityExecutableDiagnostics(null);
                        scheduleUnityExecutableValidation(selectedPath);
                      }}
                      removalCallout={
                        pendingBuildTargetRemoval ? (
                          <BuildTargetRemovalCallout
                            bindingImpact={pendingBuildTargetBindingImpact}
                            onCancel={() =>
                              setPendingBuildTargetRemovalId(null)
                            }
                            onConfirm={handleConfirmBuildTargetRemoval}
                            targetName={pendingBuildTargetRemoval.name}
                          />
                        ) : null
                      }
                      rootError={
                        attemptedSteps.targets
                          ? validationState.targets.root
                          : undefined
                      }
                      targetErrors={
                        attemptedSteps.targets
                          ? validationState.targets.targets
                          : {}
                      }
                      unityExecutableDiagnostics={unityExecutableDiagnostics}
                      unityExecutableError={
                        attemptedSteps.targets
                          ? validationState.targets.root
                          : undefined
                      }
                      unityExecutablePath={draft.unityExecutablePath}
                    />
                  ) : null}

                  {tab.key === "publish" ? (
                    <ProjectPublishStep
                      buildTargets={buildTargetReferences}
                      credentials={publishCredentials}
                      destinations={draft.publishDestinations}
                      disabled={isSaving}
                      errors={
                        attemptedSteps.publish
                          ? validationState.publish
                          : undefined
                      }
                      onChange={(nextPublishDestinations) => {
                        startTransition(() => {
                          setDraft((currentDraft) => {
                            if (!currentDraft) {
                              return currentDraft;
                            }

                            return {
                              ...currentDraft,
                              publishDestinations: nextPublishDestinations,
                            };
                          });
                        });
                      }}
                      onSaveCredential={handleSavePublishCredential}
                      showItchUserversionTemplate={
                        draft.projectKind === "repository"
                      }
                    />
                  ) : null}

                  {tab.key === "paths" ? (
                    <ProjectPathsStep
                      artifactsRootOverride={draft.artifactsRootOverride}
                      artifactsRootOverrideError={
                        shouldShowFieldError(
                          attemptedSteps.paths,
                          touchedFields,
                          "artifactsRootOverride",
                        )
                          ? validationState.paths.artifactsRootOverride
                          : undefined
                      }
                      disabled={isSaving}
                      onArtifactsRootClear={() =>
                        handleProjectPathCleared("artifactsRootOverride")
                      }
                      onArtifactsRootPicked={(selectedPath) =>
                        handleProjectPathPicked(
                          "artifactsRootOverride",
                          selectedPath,
                        )
                      }
                      onPathPickError={handlePathPickerError}
                      onWorkspaceRootClear={() =>
                        handleProjectPathCleared("workspaceRootOverride")
                      }
                      onWorkspaceRootPicked={(selectedPath) =>
                        handleProjectPathPicked(
                          "workspaceRootOverride",
                          selectedPath,
                        )
                      }
                      workspaceRootOverride={draft.workspaceRootOverride}
                      workspaceRootOverrideError={
                        shouldShowFieldError(
                          attemptedSteps.paths,
                          touchedFields,
                          "workspaceRootOverride",
                        )
                          ? validationState.paths.workspaceRootOverride
                          : undefined
                      }
                    />
                  ) : null}
                </ProjectDetailStepPanel>
              ))}
            </div>
          </fieldset>
        </div>
      </FocusPageFrame>
    </div>
  );
}

function disconnectCredentialIfBound(
  repositoryId: number,
  persistedRepositoryCredentialId: number | null,
) {
  if (persistedRepositoryCredentialId === null) {
    return Promise.resolve();
  }

  return disconnectRepositoryAuth(repositoryId);
}

function ProjectRemovalOverlay({
  hasPendingChanges,
  onResolve,
  projectName,
}: ProjectRemovalOverlayProps) {
  const { t } = useLocalization();
  const resolvedProjectName = projectName.trim() || "this project";

  return (
    <FullScreenModal
      className="project-removal-dialog__modal"
      description={t(
        "project_detail.remove.description",
        "Choose whether HGP should only remove the project from SQLite or also purge runtime-owned files from disk.",
      )}
      onResolve={onResolve}
      title={t("project_detail.remove.title", "Remove {{projectName}}?", {
        projectName: resolvedProjectName,
      })}
    >
      <div className="project-removal-dialog">
        <p className="project-removal-dialog__copy">
          {hasPendingChanges
            ? t(
                "project_detail.remove.pending_changes",
                "Unsaved edits will be discarded when the project is removed.",
              )
            : t(
                "project_detail.remove.copy",
                "Select how thoroughly HGP should remove this project from the app.",
              )}
        </p>

        <div className="project-removal-dialog__options">
          <section className="project-removal-dialog__option">
            <div>
              <h3 className="project-removal-dialog__option-title">
                {t(
                  "project_detail.remove.detach.title",
                  "Remove from App Only",
                )}
              </h3>
              <p className="project-removal-dialog__option-copy">
                {t(
                  "project_detail.remove.detach.copy",
                  "Deletes the project from SQLite and keeps workspaces, artifacts, logs, and retained files on disk.",
                )}
              </p>
            </div>

            <div className="project-removal-dialog__option-actions">
              <Button
                onClick={() => onResolve?.("detach")}
                size="sm"
                variant="primary"
              >
                {t(
                  "project_detail.remove.detach.action",
                  "Remove from App Only",
                )}
              </Button>
            </div>
          </section>

          <section className="project-removal-dialog__option">
            <div>
              <h3 className="project-removal-dialog__option-title">
                {t("project_detail.remove.purge.title", "Purge Total")}
              </h3>
              <p className="project-removal-dialog__option-copy">
                {t(
                  "project_detail.remove.purge.copy",
                  "Deletes the project from SQLite and removes runtime-owned workspaces, artifacts, logs, and retained files collected for this project.",
                )}
              </p>
            </div>

            <div className="project-removal-dialog__option-actions">
              <Button
                className="project-removal-dialog__action--purge"
                onClick={() => onResolve?.("purge")}
                size="sm"
                variant="secondary"
              >
                {t("project_detail.remove.purge.action", "Purge Total")}
              </Button>
            </div>
          </section>
        </div>

        <div className="confirm-dialog__actions">
          <Button
            data-overlay-autofocus
            onClick={() => onResolve?.(null)}
            size="sm"
            variant="ghost"
          >
            {t("project_detail.actions.cancel", "Cancel")}
          </Button>
        </div>
      </div>
    </FullScreenModal>
  );
}

function ProjectDetailSectionTabs({
  activeSection,
  disabled = false,
  onChange,
  tabs,
}: {
  activeSection: WizardStepKey;
  disabled?: boolean;
  onChange: (sectionKey: WizardStepKey) => void;
  tabs: ProjectDetailTab[];
}) {
  const { t } = useLocalization();
  const tabRefs = useRef<
    Partial<Record<WizardStepKey, HTMLButtonElement | null>>
  >({});

  const handleTabKeyDown =
    (key: WizardStepKey) => (event: React.KeyboardEvent<HTMLButtonElement>) => {
      if (disabled) {
        return;
      }

      const currentIndex = tabs.findIndex((tab) => tab.key === key);
      if (currentIndex < 0) {
        return;
      }

      let nextIndex: number | null = null;

      switch (event.key) {
        case "ArrowUp":
          nextIndex = currentIndex === 0 ? tabs.length - 1 : currentIndex - 1;
          break;
        case "ArrowDown":
          nextIndex = currentIndex === tabs.length - 1 ? 0 : currentIndex + 1;
          break;
        case "Home":
          nextIndex = 0;
          break;
        case "End":
          nextIndex = tabs.length - 1;
          break;
        default:
          return;
      }

      event.preventDefault();
      const nextTab = tabs[nextIndex];
      onChange(nextTab.key);
      window.setTimeout(() => {
        tabRefs.current[nextTab.key]?.focus();
      }, 0);
    };

  return (
    <div
      aria-label={t(
        "project_detail.sections.aria_label",
        "Project detail sections",
      )}
      aria-orientation="vertical"
      className="project-detail-tablist ui-panel ui-panel--inset"
      role="tablist"
    >
      {tabs.map((tab) => {
        const isActive = tab.key === activeSection;

        return (
          <IconButton
            key={tab.key}
            aria-controls={`project-detail-panel-${tab.key}`}
            aria-disabled={disabled}
            aria-selected={isActive}
            className={joinClassNames(
              "project-detail-tab",
              isActive && "project-detail-tab--active",
            )}
            disabled={disabled}
            icon={tab.icon}
            id={`project-detail-tab-${tab.key}`}
            label={tab.label}
            onClick={() => onChange(tab.key)}
            onKeyDown={handleTabKeyDown(tab.key)}
            ref={(node) => {
              tabRefs.current[tab.key] = node;
            }}
            role="tab"
            size="sm"
            tabIndex={isActive ? 0 : -1}
            variant={isActive ? "primary" : "ghost"}
          />
        );
      })}
    </div>
  );
}

function ProjectDetailStepPanel({
  children,
  description,
  open,
  sectionKey,
  summary,
  title,
}: {
  children: ReactNode;
  description: string;
  open: boolean;
  sectionKey: WizardStepKey;
  summary?: ReactNode;
  title: string;
}) {
  if (!open) {
    return null;
  }

  return (
    <section
      aria-labelledby={`project-detail-tab-${sectionKey}`}
      className="ui-panel ui-panel--section ui-panel--header-separated project-detail-section-panel"
      id={`project-detail-panel-${sectionKey}`}
      role="tabpanel"
    >
      <div className="ui-panel__header project-detail-section-panel__header">
        <div className="project-detail-section-accordion__header-content">
          <div className="ui-panel__title-block">
            <p className="ui-panel__eyebrow">{title}</p>
            <h2 className="ui-panel__title">{title}</h2>
            <p className="ui-panel__description">{description}</p>
            {summary ? (
              <SummaryStrip className="project-detail-section-accordion__summary">
                {summary}
              </SummaryStrip>
            ) : null}
          </div>
        </div>
      </div>
      <div className="ui-panel__body project-detail-section-panel__body">
        {children}
      </div>
    </section>
  );
}

function buildProjectDetailTabs(steps: ReturnType<typeof buildWizardSteps>) {
  const iconByKey: Record<WizardStepKey, IconName> = {
    identity: "settings",
    access: "layout",
    targets: "box",
    publish: "arrowUpRight",
    paths: "folder",
    review: "checkCircle",
  };

  return steps
    .filter((step) => step.key !== "review")
    .map((step) => ({
      description: step.description,
      icon: iconByKey[step.key],
      key: step.key,
      label: step.label,
    }));
}

function buildStepSummary(
  stepKey: WizardStepKey,
  draft: ProjectDraft,
  repositoryAccessSummary: string,
  t?: Translate,
) {
  switch (stepKey) {
    case "identity":
      return (
        <MetaRow>
          <MetaItem
            label={translateMessage(
              t,
              "project_detail.step.identity.name",
              "Name",
            )}
          >
            {draft.name.trim() ||
              translateMessage(
                t,
                "project_detail.step.identity.unnamed",
                "Unnamed",
              )}
          </MetaItem>
          <MetaItem
            label={translateMessage(
              t,
              "project_detail.step.identity.kind",
              "Kind",
            )}
          >
            {formatProjectKindLabel(draft.projectKind, t)}
          </MetaItem>
          <MetaItem
            label={translateMessage(
              t,
              "project_detail.step.identity.engine",
              "Engine",
            )}
          >
            {formatRepositoryEngineKindLabel(draft.engineKind, t)}
          </MetaItem>
        </MetaRow>
      );
    case "access":
      return draft.projectKind === "repository" ? (
        <MetaRow>
          <MetaItem
            label={translateMessage(
              t,
              "project_detail.step.access.visibility",
              "Visibility",
            )}
          >
            {draft.repositoryVisibility === "private"
              ? translateMessage(
                  t,
                  "project_shared.repository_visibility.private",
                  "Private",
                )
              : translateMessage(
                  t,
                  "project_shared.repository_visibility.public",
                  "Public",
                )}
          </MetaItem>
          <MetaItem
            label={translateMessage(
              t,
              "project_detail.step.access.poll",
              "Poll",
            )}
          >
            {`${draft.pollingIntervalSeconds.trim() || "0"}s`}
          </MetaItem>
          <MetaItem
            label={translateMessage(
              t,
              "project_detail.step.access.access",
              "Access",
            )}
          >
            {repositoryAccessSummary}
          </MetaItem>
        </MetaRow>
      ) : (
        <MetaRow>
          <MetaItem
            label={translateMessage(
              t,
              "project_detail.step.access.workspace",
              "Workspace",
            )}
          >
            {draft.localPath.trim()
              ? translateMessage(
                  t,
                  "project_detail.state.configured",
                  "Configured",
                )
              : translateMessage(t, "project_detail.state.missing", "Missing")}
          </MetaItem>
        </MetaRow>
      );
    case "targets":
      return (
        <MetaRow>
          <MetaItem
            label={translateMessage(
              t,
              "project_detail.step.targets.count",
              "Targets",
            )}
          >
            {draft.buildTargets.length}
          </MetaItem>
          <MetaItem
            label={translateMessage(
              t,
              "project_detail.step.targets.executable",
              "Executable",
            )}
          >
            {draft.unityExecutablePath.trim()
              ? translateMessage(
                  t,
                  "project_detail.state.configured",
                  "Configured",
                )
              : translateMessage(
                  t,
                  "project_shared.repository_access.summary.pending",
                  "Pending",
                )}
          </MetaItem>
        </MetaRow>
      );
    case "publish":
      return (
        <MetaRow>
          <MetaItem
            label={translateMessage(
              t,
              "project_detail.step.publish.destinations",
              "Destinations",
            )}
          >
            {draft.publishDestinations.length}
          </MetaItem>
        </MetaRow>
      );
    case "paths":
      return (
        <MetaRow>
          <MetaItem
            label={translateMessage(
              t,
              "project_detail.step.paths.artifacts",
              "Artifacts",
            )}
          >
            {draft.artifactsRootOverride.trim()
              ? translateMessage(t, "project_detail.state.override", "Override")
              : translateMessage(t, "project_detail.state.default", "Default")}
          </MetaItem>
          <MetaItem
            label={translateMessage(
              t,
              "project_detail.step.paths.workspace",
              "Workspace",
            )}
          >
            {draft.workspaceRootOverride.trim()
              ? translateMessage(t, "project_detail.state.override", "Override")
              : translateMessage(t, "project_detail.state.default", "Default")}
          </MetaItem>
        </MetaRow>
      );
    case "review":
      return (
        <MetaRow>
          <MetaItem
            label={translateMessage(
              t,
              "project_detail.step.review.ready",
              "Ready",
            )}
          >
            {translateMessage(
              t,
              "project_detail.step.review.copy",
              "Review before save",
            )}
          </MetaItem>
        </MetaRow>
      );
  }
}

function createEmptyValidationState(): ValidationState {
  return {
    identity: {},
    access: {},
    targets: {
      targets: {},
    },
    publish: {
      destinations: {},
    },
    paths: {},
  };
}

function buildValidationState(input: {
  draft: ProjectDraft;
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
  unityExecutableDiagnostics: UnityExecutableValidation | null;
  isValidatingUnityExecutable: boolean;
  inventory: RepositoryInspectionEntry[];
  t?: Translate;
}): ValidationState {
  const identity = validateIdentityStep(input.draft, input.inventory, input.t);
  const access = validateAccessStep(
    input.draft,
    {
      repositoryAccessAssessment: input.repositoryAccessAssessment,
      isAssessingRepositoryAccess: input.isAssessingRepositoryAccess,
      repositoryAccessError: input.repositoryAccessError,
      repositoryCredentialId: input.repositoryCredentialId,
      githubAuthProvider: input.githubAuthProvider,
      isLoadingAuthProviders: input.isLoadingAuthProviders,
      authProviderError: input.authProviderError,
      isLoadingRepositoryCredentials: input.isLoadingRepositoryCredentials,
      repositoryCredentialsError: input.repositoryCredentialsError,
      repositoryCredentialCount: input.repositoryCredentialCount,
    },
    input.t,
  );
  const buildTargetAdapter = resolveBuildTargetWizardAdapter(
    input.draft.engineKind,
    input.draft.projectKind,
    input.t,
  );
  const targets = validateTargetsStep(
    input.draft,
    input.unityExecutableDiagnostics,
    input.isValidatingUnityExecutable,
    buildTargetAdapter,
    input.t,
  );
  const publish = validatePublishDestinationDrafts(
    input.draft.publishDestinations,
    input.draft.buildTargets.map((target) => ({
      id: target.id,
      buildTargetId: target.buildTargetId ?? null,
      name:
        target.name.trim() ||
        translateMessage(
          input.t,
          "project_shared.target.unnamed",
          "Unnamed target",
        ),
    })),
  );
  const paths = validatePathStep(input.draft, input.t);

  return {
    identity,
    access,
    targets,
    publish,
    paths,
  };
}

function buildAttemptedStepsForValidation(validationState: ValidationState) {
  return {
    identity: Boolean(
      validationState.identity.name ||
      validationState.identity.projectKind ||
      validationState.identity.engineKind,
    ),
    access: Boolean(
      validationState.access.repositoryUrl ||
      validationState.access.localPath ||
      validationState.access.pollingIntervalSeconds ||
      validationState.access.repositoryAccess,
    ),
    targets: Boolean(
      validationState.targets.root ||
      Object.values(validationState.targets.targets).some((fieldErrors) =>
        Boolean(
          fieldErrors.name ||
          fieldErrors.targetPlatform ||
          fieldErrors.buildMethod,
        ),
      ),
    ),
    publish: hasPublishDestinationValidationErrors(validationState.publish),
    paths: Boolean(
      validationState.paths.artifactsRootOverride ||
      validationState.paths.workspaceRootOverride,
    ),
    review: false,
  };
}

function findFirstInvalidStep(
  validationState: ValidationState,
): WizardStepKey | null {
  if (
    validationState.identity.name ||
    validationState.identity.projectKind ||
    validationState.identity.engineKind
  ) {
    return "identity";
  }

  if (
    validationState.access.repositoryUrl ||
    validationState.access.localPath ||
    validationState.access.pollingIntervalSeconds ||
    validationState.access.repositoryAccess
  ) {
    return "access";
  }

  if (
    validationState.targets.root ||
    Object.values(validationState.targets.targets).some((fieldErrors) =>
      Boolean(
        fieldErrors.name ||
        fieldErrors.targetPlatform ||
        fieldErrors.buildMethod,
      ),
    )
  ) {
    return "targets";
  }

  if (hasPublishDestinationValidationErrors(validationState.publish)) {
    return "publish";
  }

  if (
    validationState.paths.artifactsRootOverride ||
    validationState.paths.workspaceRootOverride
  ) {
    return "paths";
  }

  return null;
}

function validateIdentityStep(
  draft: ProjectDraft,
  inventory: RepositoryInspectionEntry[],
  t?: Translate,
) {
  const errors: ValidationState["identity"] = {};
  const normalizedName = draft.name.trim();

  if (!normalizedName) {
    errors.name = translateMessage(
      t,
      "create_project.validation.identity.name_required",
      "Project name is required.",
    );
  } else if (
    inventory.some(
      (entry) =>
        entry.repository_name.trim().toLocaleLowerCase() ===
        normalizedName.toLocaleLowerCase(),
    )
  ) {
    errors.name = translateMessage(
      t,
      "project_detail.validation.identity.name_duplicate",
      "Another project already uses this name.",
    );
  }

  if (draft.engineKind !== "unity") {
    errors.engineKind = translateMessage(
      t,
      "project_detail.validation.identity.engine_unsupported",
      "Only Unity is currently supported even though future engines are listed.",
    );
  }

  return errors;
}

function validateAccessStep(
  draft: ProjectDraft,
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
  t?: Translate,
) {
  const errors: ValidationState["access"] = {};

  if (draft.projectKind === "local") {
    const localPath = draft.localPath.trim();

    if (!localPath) {
      errors.localPath = translateMessage(
        t,
        "create_project.validation.access.local_path_required",
        "Local workspace path is required.",
      );
    } else if (!looksLikeAbsolutePath(localPath)) {
      errors.localPath = translateMessage(
        t,
        "create_project.validation.access.local_path_absolute",
        "Local workspace path must be an absolute path.",
      );
    }

    return errors;
  }

  const repositoryUrl = draft.repositoryUrl.trim();
  if (!repositoryUrl) {
    errors.repositoryUrl = translateMessage(
      t,
      "create_project.validation.access.repository_url_required",
      "Repository URL is required.",
    );
  } else if (
    !(
      repositoryUrl.startsWith("https://") ||
      repositoryUrl.startsWith("http://")
    )
  ) {
    errors.repositoryUrl = translateMessage(
      t,
      "create_project.validation.access.repository_url_protocol",
      "Repository URL must use http:// or https://.",
    );
  }

  const pollingInterval = Number(draft.pollingIntervalSeconds.trim());
  if (!Number.isInteger(pollingInterval)) {
    errors.pollingIntervalSeconds = translateMessage(
      t,
      "create_project.validation.access.polling_integer",
      "Polling interval must be a whole number.",
    );
  } else if (pollingInterval < MIN_PROJECT_POLL_INTERVAL_SECONDS) {
    errors.pollingIntervalSeconds = translateMessage(
      t,
      "create_project.validation.access.polling_minimum",
      "Polling interval must be at least 5 seconds.",
    );
  }

  if (!errors.repositoryUrl) {
    if (authState.isAssessingRepositoryAccess) {
      errors.repositoryAccess = translateMessage(
        t,
        "create_project.validation.access.repository_checking",
        "Repository access is still being checked.",
      );
    } else if (authState.repositoryAccessError) {
      errors.repositoryAccess = translateMessage(
        t,
        "create_project.validation.access.repository_check_failed",
        "Repository access could not be checked from the desktop shell.",
      );
    } else if (!authState.repositoryAccessAssessment) {
      errors.repositoryAccess = translateMessage(
        t,
        "create_project.validation.access.repository_not_checked",
        "Repository access has not been checked yet.",
      );
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
        errors.repositoryAccess = translateMessage(
          t,
          "project_detail.validation.access.private_unsupported_host",
          "Private repositories are not supported for this host yet. Only public repositories can be saved right now.",
        );
      } else if (authState.isLoadingRepositoryCredentials) {
        errors.repositoryAccess = translateMessage(
          t,
          "create_project.validation.access.credentials_loading",
          "Repository credentials are still loading from the desktop shell.",
        );
      } else if (authState.repositoryCredentialsError) {
        errors.repositoryAccess = translateMessage(
          t,
          "create_project.validation.access.credentials_error",
          "Repository credentials could not be loaded from the desktop shell.",
        );
      } else if (!authState.repositoryCredentialId) {
        if (
          authState.repositoryAccessAssessment.provider_id === "github" &&
          authState.repositoryAccessAssessment.supports_interactive_login
        ) {
          if (authState.isLoadingAuthProviders) {
            errors.repositoryAccess = translateMessage(
              t,
              "create_project.validation.access.github_loading",
              "GitHub login status is still loading.",
            );
          } else if (authState.authProviderError) {
            errors.repositoryAccess = translateMessage(
              t,
              "create_project.validation.access.github_error",
              "GitHub login status could not be loaded from the desktop shell.",
            );
          } else if (authState.githubAuthProvider?.status !== "connected") {
            errors.repositoryAccess = translateMessage(
              t,
              "project_detail.validation.access.github_login_required",
              "Private GitHub repository detected. Log in and connect a GitHub credential, or select another stored repository credential, before saving.",
            );
          } else {
            errors.repositoryAccess = translateMessage(
              t,
              "project_detail.validation.access.github_connect_required",
              "Private GitHub repository detected. Connect a credential to this project before saving.",
            );
          }
        } else if (authState.repositoryCredentialCount === 0) {
          errors.repositoryAccess = translateMessage(
            t,
            "project_detail.validation.access.credentials_missing",
            "No stored repository credentials are available yet. Save one before changes can continue.",
          );
        } else {
          errors.repositoryAccess = translateMessage(
            t,
            "project_detail.validation.access.private_select_credential",
            "Private repository detected. Select a stored repository credential before saving.",
          );
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
  buildTargetAdapter: ReturnType<typeof resolveBuildTargetWizardAdapter>,
  t?: Translate,
): TargetStepErrors {
  const errors: TargetStepErrors = {
    targets: {},
  };

  if (buildTargetAdapter.kind !== "unity") {
    errors.root =
      buildTargetAdapter.unsupportedMessage ||
      translateMessage(
        t,
        "project_detail.validation.targets.engine_unsupported",
        "The selected engine does not have a build target adapter yet.",
      );
    return errors;
  }

  if (draft.buildTargets.length === 0) {
    errors.root = translateMessage(
      t,
      "create_project.validation.targets.minimum_count",
      "At least one build target is required.",
    );
    return errors;
  }

  const seenNames = new Set<string>();
  const seenTargetPlatforms = new Set<string>();
  for (const target of draft.buildTargets) {
    const fieldErrors: TargetFieldErrors = {};
    const normalizedName = target.name.trim();

    if (!normalizedName) {
      fieldErrors.name = translateMessage(
        t,
        "create_project.validation.targets.name_required",
        "Target name is required.",
      );
    } else {
      const duplicateKey = normalizedName.toLocaleLowerCase();
      if (seenNames.has(duplicateKey)) {
        fieldErrors.name = translateMessage(
          t,
          "create_project.validation.targets.name_unique",
          "Target names must remain unique within the project.",
        );
      }
      seenNames.add(duplicateKey);
    }

    const normalizedTargetPlatform = normalizeUnityTargetPlatformValue(
      target.targetPlatform,
    );

    if (!normalizedTargetPlatform) {
      fieldErrors.targetPlatform = translateMessage(
        t,
        "create_project.validation.targets.platform_required",
        "Unity target platform is required.",
      );
    } else {
      if (seenTargetPlatforms.has(normalizedTargetPlatform)) {
        fieldErrors.targetPlatform = translateMessage(
          t,
          "create_project.validation.targets.platform_unique",
          "Each Unity target platform can be added only once.",
        );
      }

      seenTargetPlatforms.add(normalizedTargetPlatform);
    }

    if (!target.buildMethod.trim()) {
      fieldErrors.buildMethod = translateMessage(
        t,
        "create_project.validation.targets.build_method_required",
        "Build method is required.",
      );
    } else if (!target.buildMethod.includes(".")) {
      fieldErrors.buildMethod = translateMessage(
        t,
        "create_project.validation.targets.build_method_format",
        "Use a full static method path such as Builder.PerformWindows.",
      );
    }

    errors.targets[target.id] = fieldErrors;
  }

  if (!draft.unityExecutablePath.trim()) {
    errors.root = translateMessage(
      t,
      "create_project.validation.targets.unity_path_required",
      "Unity executable path is required for all build targets.",
    );
  } else if (isValidatingUnityExecutable) {
    errors.root = translateMessage(
      t,
      "create_project.validation.targets.unity_path_validating",
      "Unity executable validation is still running.",
    );
  } else if (!unityExecutableDiagnostics) {
    errors.root = translateMessage(
      t,
      "create_project.validation.targets.unity_path_pending",
      "Unity executable path has not been validated yet.",
    );
  } else if (unityExecutableDiagnostics.status !== "ready") {
    errors.root =
      unityExecutableDiagnostics.message ||
      translateMessage(
        t,
        "create_project.validation.targets.unity_path_invalid",
        "Unity executable path is invalid.",
      );
  }

  return errors;
}

function validatePathStep(draft: ProjectDraft, t?: Translate): PathStepErrors {
  const errors: PathStepErrors = {};
  const normalizedArtifactsRoot = draft.artifactsRootOverride.trim();
  const normalizedWorkspaceRoot = draft.workspaceRootOverride.trim();

  if (
    normalizedArtifactsRoot &&
    !looksLikeAbsolutePath(normalizedArtifactsRoot)
  ) {
    errors.artifactsRootOverride = translateMessage(
      t,
      "create_project.validation.paths.artifacts_absolute",
      "Artifacts root override must be an absolute path.",
    );
  }

  if (
    normalizedWorkspaceRoot &&
    !looksLikeAbsolutePath(normalizedWorkspaceRoot)
  ) {
    errors.workspaceRootOverride = translateMessage(
      t,
      "create_project.validation.paths.workspace_absolute",
      "Workspace root override must be an absolute path.",
    );
  }

  return errors;
}

function buildRepositoryProjectDraft(
  repository: RepositoryInspectionEntry,
): ProjectDraft {
  const buildTargets = repository.build_targets
    .filter((buildTarget) => buildTarget.enabled)
    .map((target, index) => ({
      id: `target-${index + 1}`,
      buildMethod: target.unity_build_method ?? "",
      buildTargetId: target.build_target_id,
      name: target.target_name,
      targetPlatform: normalizeUnityTargetPlatformValue(
        target.unity_target_platform,
      ),
      unityExecutablePath:
        target.host_native_diagnostics?.unity_executable_path ?? "",
    }));
  const initialUnityState = resolveInitialUnityExecutableState(buildTargets);

  return {
    projectKind:
      repository.source_mode === "local_workspace" ? "local" : "repository",
    engineKind: normalizeRepositoryEngineKind(repository.engine_kind),
    name: repository.repository_name,
    repositoryUrl:
      repository.source_mode === "managed_repository"
        ? repository.repo_url
        : "",
    localPath:
      repository.source_mode === "local_workspace"
        ? (repository.local_path ?? repository.repo_url)
        : "",
    repositoryVisibility: resolveRepositoryVisibilitySelection(repository),
    pollingIntervalSeconds:
      repository.source_mode === "managed_repository"
        ? String(repository.polling_interval_seconds)
        : "0",
    artifactsRootOverride: repository.artifacts_root_override ?? "",
    workspaceRootOverride: repository.workspace_root_override ?? "",
    unityExecutablePath: initialUnityState.sharedPath,
    buildTargets,
    publishDestinations: buildPublishDestinationDrafts(
      repository.publish_targets,
      buildTargets.map((target) => ({
        id: target.id,
        buildTargetId: target.buildTargetId ?? null,
        name: target.name.trim() || "Unnamed target",
      })),
    ),
  };
}

function buildRepositoryProjectUpdateInput(
  repository: RepositoryInspectionEntry,
  draft: ProjectDraft,
  repositoryAccessAssessment: RepositoryAccessAssessment | null,
  initialUnityExecutableState: InitialUnityExecutableState,
): UpdateRepositoryProjectInput {
  const sourceMode =
    draft.projectKind === "repository"
      ? "managed_repository"
      : "local_workspace";
  const sharedUnityPathChanged =
    draft.unityExecutablePath.trim() !==
    initialUnityExecutableState.sharedPath.trim();

  return {
    repository_id: repository.repository_id,
    source_mode: sourceMode,
    name: draft.name.trim(),
    engine_kind: draft.engineKind,
    repository_url:
      sourceMode === "managed_repository" ? draft.repositoryUrl.trim() : null,
    local_path:
      sourceMode === "local_workspace" ? draft.localPath.trim() : null,
    repository_access_assessment:
      sourceMode === "managed_repository" ? repositoryAccessAssessment : null,
    default_branch: normalizeOptionalDraftValue(
      repository.default_branch ?? "",
    ),
    artifacts_root_override: normalizeOptionalDraftValue(
      draft.artifactsRootOverride,
    ),
    workspace_root_override: normalizeOptionalDraftValue(
      draft.workspaceRootOverride,
    ),
    polling_interval_seconds:
      sourceMode === "managed_repository"
        ? Number(draft.pollingIntervalSeconds)
        : 0,
    enabled: repository.enabled,
    build_targets: draft.buildTargets.map((target) => ({
      build_target_id: target.buildTargetId ?? null,
      name: target.name.trim(),
      contract: {
        unity: {
          target_platform: normalizeUnityTargetPlatformValue(
            target.targetPlatform,
          ),
          build_method: target.buildMethod.trim(),
        },
      },
      unity_executable_path:
        sharedUnityPathChanged || !initialUnityExecutableState.hasMixedValues
          ? draft.unityExecutablePath.trim()
          : target.unityExecutablePath?.trim() ||
            draft.unityExecutablePath.trim(),
    })),
    publish_targets: buildUpdateProjectPublishTargetsInput(
      draft.publishDestinations,
      draft.buildTargets.map((target) => ({
        id: target.id,
        buildTargetId: target.buildTargetId ?? null,
        name: target.name.trim() || "Unnamed target",
      })),
    ),
  };
}

function buildProjectSourcePresentationInput(draft: ProjectDraft) {
  return {
    localPath: draft.localPath,
    repositoryUrl: draft.repositoryUrl,
    sourceMode:
      draft.projectKind === "repository"
        ? "managed_repository"
        : "local_workspace",
  };
}

function buildProjectDraftDirtyKey(draft: ProjectDraft) {
  return JSON.stringify(draft);
}

function buildProjectDetailErrorMessage(error: unknown, t?: Translate): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return translateMessage(
    t,
    "project_detail.error.load_failed",
    "The desktop shell could not load the project detail.",
  );
}

function buildProjectSaveErrorMessage(error: unknown, t?: Translate): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return translateMessage(
    t,
    "project_detail.error.save_failed",
    "The desktop shell could not save the project changes.",
  );
}

function buildProjectRemoveErrorMessage(error: unknown, t?: Translate): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return translateMessage(
    t,
    "project_detail.error.remove_failed",
    "The desktop shell could not remove the project.",
  );
}

function buildRepositoryAccessAssessmentFromRepository(
  repository: RepositoryInspectionEntry,
): RepositoryAccessAssessment | null {
  if (repository.source_mode !== "managed_repository") {
    return null;
  }

  const normalizedUrl = repository.repo_url.trim();
  if (!normalizedUrl) {
    return null;
  }

  const providerId = repository.source_provider_id ?? "unknown";

  return {
    provider_id: providerId,
    provider_label: resolveRepositoryProviderLabel(providerId),
    instance_url: repository.source_instance_url ?? "",
    normalized_url: normalizedUrl,
    visibility: repository.visibility_status,
    auth_requirement: repository.auth_requirement_status,
    auth_status: repository.auth_binding_status,
    supports_interactive_login: providerId === "github",
    message: repository.auth_status_message,
  };
}

function resolveRepositoryProviderLabel(providerId: string) {
  switch (providerId) {
    case "github":
      return "GitHub";
    case "unknown":
      return "Unknown";
    default:
      return providerId;
  }
}

function resolveRepositoryVisibilitySelection(
  repository: RepositoryInspectionEntry,
) {
  if (
    repository.visibility_status === "private" ||
    repository.auth_requirement_status === "required" ||
    repository.credentials !== null
  ) {
    return "private" as const;
  }

  return "public" as const;
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

function buildRepositoryCredentialOptions(
  credentials: SecretCredentialSetting[],
  repositoryCredentialId: number | null,
  isLoadingRepositoryCredentials: boolean,
  t?: Translate,
): SelectOption[] {
  const placeholderLabel = isLoadingRepositoryCredentials
    ? translateMessage(
        t,
        "project_shared.repository_access.option.loading",
        "Loading stored credentials...",
      )
    : credentials.length === 0
      ? translateMessage(
          t,
          "project_shared.repository_access.option.none_available",
          "No stored repository credentials available",
        )
      : translateMessage(
          t,
          "project_shared.repository_access.option.none_selected",
          "No repository credential selected",
        );

  const options: SelectOption[] = [
    {
      disabled: isLoadingRepositoryCredentials,
      label: placeholderLabel,
      value: "",
    },
    ...credentials.map((credential) => ({
      label: formatRepositoryCredentialOptionLabel(credential, t),
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
      label: translateMessage(
        t,
        "project_shared.repository_access.option.current",
        "Current credential #{{credentialId}}",
        { credentialId: repositoryCredentialId },
      ),
      value: repositoryCredentialId.toString(),
    });
  }

  return options;
}

function formatRepositoryCredentialOptionLabel(
  credential: SecretCredentialSetting,
  t?: Translate,
) {
  return `${credential.name} (${formatRepositoryCredentialKindLabel(credential.kind, t)})`;
}

function formatRepositoryCredentialKindLabel(kind: string, t?: Translate) {
  switch (kind) {
    case "git-http-basic":
      return translateMessage(
        t,
        "project_shared.repository_access.kind.http_basic",
        "HTTP basic",
      );
    case "git-http-bearer":
      return translateMessage(
        t,
        "project_shared.repository_access.kind.bearer",
        "Bearer token",
      );
    case "git-http-github-host-login":
      return translateMessage(
        t,
        "project_shared.repository_access.kind.github_login",
        "GitHub login",
      );
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

function createEmptyBuildTargetDraft(index: number): BuildTargetDraft {
  return {
    id: `target-${index}`,
    name: "",
    targetPlatform: "",
    buildMethod: "",
  };
}

function toProjectBuildTargetDraft(
  target: SharedBuildTargetDraft,
): BuildTargetDraft {
  return {
    id: target.id,
    name: target.name,
    targetPlatform: target.targetPlatform,
    buildMethod: target.buildMethod,
  };
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

function normalizeRepositoryEngineKind(_value: string): RepositoryEngineKind {
  return "unity";
}

function normalizeOptionalDraftValue(value: string) {
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function resolveInitialUnityExecutableState(buildTargets: BuildTargetDraft[]) {
  if (buildTargets.length === 0) {
    return {
      hasMixedValues: false,
      sharedPath: "",
    };
  }

  const normalizedValues = buildTargets.map((target) =>
    (target.unityExecutablePath ?? "").trim(),
  );
  const firstValue = normalizedValues[0] ?? "";
  const hasMixedValues = normalizedValues.some((value) => value !== firstValue);
  const sharedPath =
    normalizedValues.find((value) => value.length > 0) ?? firstValue;

  return {
    hasMixedValues,
    sharedPath,
  };
}

function resolveInitialUnityExecutableDiagnostics(
  buildTargets: BuildTargetDraft[],
) {
  const matchingTarget = buildTargets.find((target) =>
    Boolean(target.unityExecutablePath?.trim()),
  );

  if (!matchingTarget) {
    return null;
  }

  return {
    runner_family: "host-native",
    unity_executable_path: matchingTarget.unityExecutablePath?.trim() ?? "",
    unity_executable_exists: true,
    unity_executable_is_file: true,
    additional_argument_count: 0,
    environment_variable_count: 0,
    status: "ready",
    message: "Ready for host-native execution.",
  } satisfies UnityExecutableValidation;
}

function hasActiveRepositoryProcesses(repository: RepositoryInspectionEntry) {
  if (
    repository.running_build_runs > 0 ||
    repository.running_publish_runs > 0
  ) {
    return true;
  }

  return repository.release_queue.some(
    (release) =>
      release.build_process_active ||
      release.publish_process_active ||
      release.running_build_runs > 0 ||
      release.running_publish_runs > 0,
  );
}

function shouldShowFieldError(
  attemptedStep: boolean,
  touchedFields: Record<string, boolean>,
  fieldKey: string,
) {
  return attemptedStep || Boolean(touchedFields[fieldKey]);
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
