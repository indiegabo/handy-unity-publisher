import {
  startTransition,
  type ReactNode,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";

import { Button, IconButton } from "./Button";
import { BuildTargetRemovalCallout } from "./BuildTargetRemovalCallout";
import FullScreenModal from "./FullScreenModal";
import { type IconName } from "./Icon";
import FormSection from "./forms/FormSection";
import {
  PublishDestinationsEditor,
  buildPublishDestinationDrafts,
  buildPublishDestinationReviewSummary,
  buildUpdateProjectPublishTargetsInput,
  collectBuildTargetBindingImpact,
  createEmptyPublishDestinationValidationErrors,
  hasPublishDestinationValidationErrors,
  listUnboundBuildTargetNames,
  removeBuildTargetBindings,
  type ProjectBuildTargetReference,
  type PublishDestinationDraft,
  type PublishDestinationValidationErrors,
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
  SummaryStrip,
  SurfacePanel,
} from "./Surface";
import {
  connectRepositoryAuth,
  detectRepositoryProvider,
  disconnectRepositoryAuth,
  loadSecretSettings,
  removeRepositoryProject,
  loadRepositoryProjectDetail,
  reconnectRepositoryAuth,
  saveSecretCredential,
  updateRepositoryProject,
  validateUnityExecutablePath,
  type RepositoryAccessAssessment,
  type RepositoryEngineKind,
  type RepositoryPublishBindingInspection,
  type RepositoryInspectionEntry,
  type RepositoryProviderDetection,
  type RemoveRepositoryProjectReport,
  type RemoveRepositoryProjectStrategy,
  type SaveSecretCredentialInput,
  type SecretCredentialSetting,
  type UnityExecutableValidation,
  type UpdateRepositoryProjectInput,
} from "../services/projects";
import {
  loadAuthProviders,
  loginWithGithubAuth,
  type AuthProviderStatus,
} from "../services/auth";

type RepositoryProjectDetailProps = {
  onProjectNameResolved?: (repositoryName: string) => void;
  onProjectRemoved?: (report: RemoveRepositoryProjectReport) => void;
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
  repositoryVisibility: "public" | "private";
  defaultBranch: string;
  artifactsRootOverride: string;
  workspaceRootOverride: string;
  pollingIntervalSeconds: string;
  enabled: "enabled" | "disabled";
  buildTargets: RepositoryProjectBuildTargetDraft[];
  publishDestinations: PublishDestinationDraft[];
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
  repositoryAccess?: string;
  buildTargetsRoot?: string;
  buildTargets: Record<string, RepositoryProjectBuildTargetValidationErrors>;
  publishDestinations: PublishDestinationValidationErrors;
};

type RepositoryProjectFieldName = Exclude<
  keyof RepositoryProjectDraft,
  "buildTargets" | "publishDestinations"
>;

type ProjectDetailSectionKey =
  | "project"
  | "repository"
  | "paths"
  | "targets"
  | "destinations"
  | "automation";

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
const DEFAULT_PROJECT_DETAIL_SECTION: ProjectDetailSectionKey = "project";
const PROJECT_DETAIL_SECTION_TABS: Array<{
  key: ProjectDetailSectionKey;
  icon: IconName;
  label: string;
}> = [
  {
    key: "project",
    icon: "settings",
    label: "Project Settings",
  },
  {
    key: "repository",
    icon: "layout",
    label: "Repository",
  },
  {
    key: "paths",
    icon: "folder",
    label: "Paths",
  },
  {
    key: "targets",
    icon: "box",
    label: "Build Targets",
  },
  {
    key: "destinations",
    icon: "arrowUpRight",
    label: "Publish Destinations",
  },
  {
    key: "automation",
    icon: "server",
    label: "Runtime Status",
  },
];

export function RepositoryProjectDetail({
  onProjectNameResolved,
  onProjectRemoved,
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
  const [repositoryCredentialId, setRepositoryCredentialId] = useState<
    number | null
  >(null);
  const [repositoryAccessActionMessage, setRepositoryAccessActionMessage] =
    useState<string | null>(null);
  const [pendingRepositoryAccessAction, setPendingRepositoryAccessAction] =
    useState(false);
  const [pathDiagnostics, setPathDiagnostics] = useState<
    Record<string, UnityExecutableValidation | null>
  >({});
  const [validatingTargets, setValidatingTargets] = useState<
    Record<string, boolean>
  >({});
  const [expandedTargetIds, setExpandedTargetIds] = useState<
    Record<string, boolean>
  >({});
  const [sectionOpenState, setSectionOpenState] = useState(() =>
    buildProjectDetailSectionState(DEFAULT_PROJECT_DETAIL_SECTION),
  );
  const [pendingBuildTargetRemovalId, setPendingBuildTargetRemovalId] =
    useState<string | null>(null);
  const [isProjectRemovalOpen, setIsProjectRemovalOpen] = useState(false);
  const [isRemovingProject, setIsRemovingProject] = useState(false);
  const [projectRemovalError, setProjectRemovalError] = useState<string | null>(
    null,
  );
  const [hasLoadedAuthProviders, setHasLoadedAuthProviders] = useState(false);
  const [hasLoadedCredentials, setHasLoadedCredentials] = useState(false);
  const nextBuildTargetIdRef = useRef(1);
  const validationTimersRef = useRef<ValidationTimerMap>({});
  const validationTokenRef = useRef<Record<string, number>>({});
  const accessAssessmentTimerRef = useRef<number | undefined>(undefined);
  const accessAssessmentTokenRef = useRef(0);
  const activeSection = resolveActiveProjectDetailSection(sectionOpenState);

  const resolveRepositoryDetail = useEffectEvent(async () => {
    return loadRepositoryProjectDetail(repositoryId);
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
        setRepositoryAccessAssessment(
          matchingRepository
            ? buildRepositoryAccessAssessmentFromRepository(matchingRepository)
            : null,
        );
        setIsAssessingRepositoryAccess(false);
        setRepositoryAccessError(null);
        setRepositoryCredentialId(
          matchingRepository?.credentials?.credential_id ?? null,
        );
        setRepositoryAccessActionMessage(null);
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
    startTransition(() => {
      setSectionOpenState(
        buildProjectDetailSectionState(DEFAULT_PROJECT_DETAIL_SECTION),
      );
      setPendingBuildTargetRemovalId(null);
      setIsProjectRemovalOpen(false);
      setIsRemovingProject(false);
      setProjectRemovalError(null);
      setHasLoadedAuthProviders(false);
      setHasLoadedCredentials(false);
      setGithubAuthProvider(null);
      setAuthProviderError(null);
      setIsLoadingAuthProviders(false);
      setRepositoryCredentials([]);
      setPublishCredentials([]);
      setRepositoryCredentialsError(null);
      setIsLoadingRepositoryCredentials(false);
    });

    void loadRepositoryDetail(true);

    return () => {
      for (const timerId of Object.values(validationTimersRef.current)) {
        if (timerId !== undefined) {
          window.clearTimeout(timerId);
        }
      }

      if (accessAssessmentTimerRef.current !== undefined) {
        window.clearTimeout(accessAssessmentTimerRef.current);
      }
    };
  }, [repositoryId]);

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
        setAuthProviderError(buildProjectSaveErrorMessage(loadError));
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
        setRepositoryCredentialsError(buildProjectSaveErrorMessage(loadError));
        setIsLoadingRepositoryCredentials(false);
      });
    }
  });

  useEffect(() => {
    if (!sectionOpenState.repository || hasLoadedAuthProviders) {
      return;
    }

    setHasLoadedAuthProviders(true);
    void loadAuthProvidersEffect();
  }, [
    hasLoadedAuthProviders,
    loadAuthProvidersEffect,
    sectionOpenState.repository,
  ]);

  useEffect(() => {
    if (
      hasLoadedCredentials ||
      (!sectionOpenState.repository && !sectionOpenState.destinations)
    ) {
      return;
    }

    setHasLoadedCredentials(true);
    void loadRepositoryCredentialsEffect();
  }, [
    hasLoadedCredentials,
    loadRepositoryCredentialsEffect,
    sectionOpenState.destinations,
    sectionOpenState.repository,
  ]);

  const loadRepositoryAccessAssessmentEffect = useEffectEvent(
    async (
      repositoryUrl: string,
      repositoryVisibility: RepositoryProjectDraft["repositoryVisibility"],
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
      } catch (assessmentError) {
        if (accessAssessmentTokenRef.current !== assessmentToken) {
          return;
        }

        startTransition(() => {
          setRepositoryAccessAssessment(null);
          setRepositoryAccessError(
            buildProjectSaveErrorMessage(assessmentError),
          );
          setIsAssessingRepositoryAccess(false);
        });
      }
    },
  );

  useEffect(() => {
    if (!sectionOpenState.repository) {
      if (accessAssessmentTimerRef.current !== undefined) {
        window.clearTimeout(accessAssessmentTimerRef.current);
        accessAssessmentTimerRef.current = undefined;
      }

      accessAssessmentTokenRef.current += 1;
      startTransition(() => {
        setIsAssessingRepositoryAccess(false);
        setRepositoryAccessError(null);
        setRepositoryAccessActionMessage(null);
      });
      return;
    }

    const repositoryUrl = draft?.repositoryUrl ?? "";
    const repositoryVisibility = draft?.repositoryVisibility ?? "public";
    const normalizedUrl = repositoryUrl.trim();
    const persistedAssessment = repository
      ? buildRepositoryAccessAssessmentFromRepository(repository)
      : null;

    if (
      repository &&
      persistedAssessment &&
      normalizedUrl === repository.repo_url.trim() &&
      repositoryVisibility === resolveRepositoryVisibilitySelection(repository)
    ) {
      if (accessAssessmentTimerRef.current !== undefined) {
        window.clearTimeout(accessAssessmentTimerRef.current);
        accessAssessmentTimerRef.current = undefined;
      }

      accessAssessmentTokenRef.current += 1;
      startTransition(() => {
        setRepositoryAccessAssessment((current) =>
          areRepositoryAccessAssessmentsEqual(current, persistedAssessment)
            ? current
            : persistedAssessment,
        );
        setRepositoryAccessError(null);
        setIsAssessingRepositoryAccess(false);
        setRepositoryAccessActionMessage(null);
      });
      return;
    }

    if (
      !normalizedUrl ||
      !(
        normalizedUrl.startsWith("https://") ||
        normalizedUrl.startsWith("http://")
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
  }, [
    repository,
    draft?.repositoryUrl,
    draft?.repositoryVisibility,
    loadRepositoryAccessAssessmentEffect,
    sectionOpenState.repository,
  ]);

  useEffect(() => {
    if (supportsShellRepositoryLoginAction(repositoryAccessAssessment)) {
      return;
    }

    startTransition(() => {
      setRepositoryCredentialId(null);
    });
  }, [repositoryAccessAssessment?.auth_requirement]);

  useEffect(() => {
    const resolvedName =
      draft?.name.trim() || repository?.repository_name.trim();

    if (!resolvedName) {
      return;
    }

    onProjectNameResolved?.(resolvedName);
  }, [draft?.name, onProjectNameResolved, repository?.repository_name]);

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
    const nextTarget = createEmptyBuildTargetDraft(
      nextBuildTargetIdRef.current,
    );
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
      setSectionOpenState(buildProjectDetailSectionState("targets"));
    });
  });

  const finalizeBuildTargetRemoval = useEffectEvent((targetId: string) => {
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
          publishDestinations: removeBuildTargetBindings(
            currentDraft.publishDestinations,
            targetId,
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

  const handleRemoveBuildTarget = useEffectEvent((targetId: string) => {
    if (!draft) {
      return;
    }

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

  const handleSectionTabChange = useEffectEvent(
    (sectionKey: ProjectDetailSectionKey) => {
      if (repository && hasActiveRepositoryProcesses(repository)) {
        return;
      }

      startTransition(() => {
        setSectionOpenState(buildProjectDetailSectionState(sectionKey));
      });
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
      setSaveError(null);
    });

    try {
      const forceBrowserLogin =
        repository?.auth_binding_status === "reauth_required";
      let provider = githubAuthProvider;
      if (
        forceBrowserLogin ||
        provider?.status !== "connected" ||
        !provider.credential_id
      ) {
        provider = await loginWithGithubAuth({ force: forceBrowserLogin });
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
          forceBrowserLogin
            ? "GitHub login refreshed for this project. Save changes to keep the connection."
            : "GitHub login connected for this project. Save changes to keep the connection.",
        );
      });
    } catch (bindingError) {
      startTransition(() => {
        setSaveError(buildProjectSaveErrorMessage(bindingError));
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
        "Repository credential cleared from the draft. Save changes to keep it disconnected.",
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
            ? "Stored repository credential selected for this project. Save changes to keep the connection."
            : "Repository credential cleared from the draft. Save changes to keep it disconnected.",
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
        if (!createdCredential) {
          throw new Error(
            "The saved publish credential could not be reloaded.",
          );
        }

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
                        credentialsId: createdCredential.credential_id,
                      }
                    : destination,
              ),
            };
          });
        });

        return createdCredential.credential_id;
      } catch (error) {
        throw new Error(buildProjectSaveErrorMessage(error));
      }
    },
  );

  const handleSaveProject = useEffectEvent(async () => {
    if (
      !repository ||
      !draft ||
      isSaving ||
      isRemovingProject ||
      hasActiveRepositoryProcesses(repository)
    ) {
      return;
    }

    const repositoryCredentialIdForSave = resolveRepositoryCredentialIdForSave(
      repositoryAccessAssessment,
      repositoryCredentialId,
    );

    const nextValidationErrors = validateRepositoryProjectDraft(
      draft,
      pathDiagnostics,
      validatingTargets,
      {
        repositoryAccessAssessment,
        isAssessingRepositoryAccess,
        repositoryAccessError,
        repositoryCredentialId: repositoryCredentialIdForSave,
        githubAuthProvider,
        isLoadingAuthProviders,
        authProviderError,
        isLoadingRepositoryCredentials,
        repositoryCredentialsError,
        repositoryCredentialCount: repositoryCredentials.length,
      },
    );
    if (hasValidationErrors(nextValidationErrors)) {
      const invalidTargetIds = collectInvalidTargetIds(nextValidationErrors);
      const hasPublishErrors = hasPublishDestinationValidationErrors(
        nextValidationErrors.publishDestinations,
      );

      startTransition(() => {
        setValidationErrors(nextValidationErrors);
        setSaveError(null);
        setSaveMessage(null);
        setExpandedTargetIds((current) =>
          mergeExpandedTargetIds(current, invalidTargetIds),
        );
        setSectionOpenState(
          buildProjectDetailSectionState(
            resolveInvalidProjectDetailSection(
              nextValidationErrors,
              invalidTargetIds,
              hasPublishErrors,
            ),
          ),
        );
      });
      return;
    }

    setIsSaving(true);
    setSaveError(null);
    setSaveMessage(null);

    try {
      await updateRepositoryProject(
        buildRepositoryProjectUpdateInput(
          repository.repository_id,
          draft,
          repositoryAccessAssessment,
        ),
      );

      const persistedRepositoryCredentialId =
        repository.credentials?.credential_id ?? null;
      if (repositoryCredentialIdForSave === null) {
        await disconnectRepositoryAuth(repository.repository_id);
      } else if (persistedRepositoryCredentialId === null) {
        await connectRepositoryAuth(
          repository.repository_id,
          repositoryCredentialIdForSave,
        );
      } else {
        await reconnectRepositoryAuth(
          repository.repository_id,
          repositoryCredentialIdForSave,
        );
      }

      const refreshedRepository = await resolveRepositoryDetail();
      if (!refreshedRepository) {
        throw new Error("The updated project could not be reloaded.");
      }

      const targetEditorState =
        buildRepositoryProjectTargetEditorState(refreshedRepository);
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
        setSaveMessage(
          `Saved changes for ${refreshedRepository.repository_name}.`,
        );
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

  const handleOpenProjectRemoval = useEffectEvent(() => {
    if (!repository || isSaving || isRemovingProject || isEditingLocked) {
      return;
    }

    startTransition(() => {
      setProjectRemovalError(null);
      setIsProjectRemovalOpen(true);
    });
  });

  const handleCloseProjectRemoval = useEffectEvent(() => {
    if (isRemovingProject) {
      return;
    }

    startTransition(() => {
      setProjectRemovalError(null);
      setIsProjectRemovalOpen(false);
    });
  });

  const handleRemoveProject = useEffectEvent(
    async (strategy: RemoveRepositoryProjectStrategy) => {
      if (!repository || isRemovingProject) {
        return;
      }

      setIsRemovingProject(true);
      setProjectRemovalError(null);

      try {
        const report = await removeRepositoryProject({
          repository_id: repository.repository_id,
          strategy,
        });

        startTransition(() => {
          setProjectRemovalError(null);
          setIsProjectRemovalOpen(false);
        });

        onProjectRemoved?.(report);
      } catch (removeProjectError) {
        startTransition(() => {
          setProjectRemovalError(
            buildProjectRemoveErrorMessage(removeProjectError),
          );
        });
      } finally {
        startTransition(() => {
          setIsRemovingProject(false);
        });
      }
    },
  );

  if (isLoading) {
    return (
      <div className="project-detail-shell">
        <FocusPageFrame
          description="Inspect and edit repository identity, build targets, and runtime-managed paths."
          eyebrow="Repository Project"
          title="Project Detail"
        >
          <div className="feed-state">
            <p className="feed-state__title">Loading project detail...</p>
            <p className="feed-state__copy">
              The shell is resolving the repository configuration that was just
              created.
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
          description="Inspect and edit repository identity, build targets, and runtime-managed paths."
          eyebrow="Repository Project"
          title="Project Detail"
        >
          <div className="feed-state">
            <p className="feed-state__title">Project detail is unavailable.</p>
            <p className="feed-state__copy">{error}</p>
            <Button
              leadingIcon="refresh"
              onClick={handleReloadProject}
              size="sm"
              variant="secondary"
            >
              Retry project load
            </Button>
          </div>
        </FocusPageFrame>
      </div>
    );
  }

  if (!repository) {
    return (
      <div className="project-detail-shell">
        <FocusPageFrame
          description="Inspect and edit repository identity, build targets, and runtime-managed paths."
          eyebrow="Repository Project"
          title="Project Detail"
        >
          <div className="feed-state">
            <p className="feed-state__title">Project not found.</p>
            <p className="feed-state__copy">
              The repository was created, but the current inspection payload
              does not include it yet.
            </p>
          </div>
        </FocusPageFrame>
      </div>
    );
  }

  if (!draft) {
    return (
      <div className="project-detail-shell">
        <FocusPageFrame
          description="Inspect and edit repository identity, build targets, and runtime-managed paths."
          eyebrow="Repository Project"
          title="Project Detail"
        >
          <div className="feed-state">
            <p className="feed-state__title">Draft state unavailable.</p>
            <p className="feed-state__copy">
              Reload the project detail to rebuild the editable draft from the
              persisted repository snapshot.
            </p>
          </div>
        </FocusPageFrame>
      </div>
    );
  }

  const hasUnsavedChanges =
    repository && draft
      ? isRepositoryProjectDraftChanged(repository, draft)
      : false;
  const desiredRepositoryCredentialId = resolveRepositoryCredentialIdForSave(
    repositoryAccessAssessment,
    repositoryCredentialId,
  );
  const repositoryCredentialOptions = buildRepositoryCredentialOptions(
    repositoryCredentials,
    desiredRepositoryCredentialId,
    isLoadingRepositoryCredentials,
  );
  const hasRepositoryCredentialChanges = repository
    ? (repository.credentials?.credential_id ?? null) !==
      desiredRepositoryCredentialId
    : false;
  const hasPendingChanges = hasUnsavedChanges || hasRepositoryCredentialChanges;
  const activeTargetCount = draft?.buildTargets.length ?? 0;
  const validatingTargetCount =
    Object.values(validatingTargets).filter(Boolean).length;
  const buildTargetReferences: ProjectBuildTargetReference[] =
    draft?.buildTargets.map((target) => ({
      id: target.id,
      buildTargetId: target.buildTargetId,
      name: target.name.trim() || "Unnamed target",
    })) ?? [];
  const targetAttentionCount = draft
    ? draft.buildTargets.filter((target) => {
        const diagnostics = pathDiagnostics[target.id];

        return diagnostics !== null && diagnostics.status !== "ready";
      }).length
    : 0;
  const publishDestinationCount = draft?.publishDestinations.length ?? 0;
  const publishDestinationReviewSummary = draft
    ? buildPublishDestinationReviewSummary(
        draft.publishDestinations,
        buildTargetReferences,
      )
    : [];
  const unboundPublishTargetNames = draft
    ? listUnboundBuildTargetNames(
        draft.publishDestinations,
        buildTargetReferences,
      )
    : [];
  const enabledNonConsumingBindingCount = draft
    ? countEnabledPublishDestinationBindings(
        draft.publishDestinations,
        "non_consuming",
      )
    : 0;
  const enabledConsumingBindingCount = draft
    ? countEnabledPublishDestinationBindings(
        draft.publishDestinations,
        "consuming",
      )
    : 0;
  const pendingBuildTargetRemoval = pendingBuildTargetRemovalId
    ? (draft?.buildTargets.find(
        (target) => target.id === pendingBuildTargetRemovalId,
      ) ?? null)
    : null;
  const pendingBuildTargetBindingImpact = pendingBuildTargetRemoval
    ? collectBuildTargetBindingImpact(
        draft?.publishDestinations ?? [],
        pendingBuildTargetRemoval.id,
      )
    : [];
  const pollingIntervalLabel =
    draft?.pollingIntervalSeconds.trim() ||
    String(repository.polling_interval_seconds);
  const runningWorkCount =
    repository.running_build_runs + repository.running_publish_runs;
  const isEditingLocked = hasActiveRepositoryProcesses(repository);
  const isProjectMutationPending = isSaving || isRemovingProject;

  return (
    <div className="project-detail-shell">
      <FocusPageFrame
        actions={
          <div className="project-detail-toolbar">
            <Button
              className="project-detail-toolbar__remove"
              disabled={isProjectMutationPending || isEditingLocked}
              leadingIcon="trash"
              onClick={handleOpenProjectRemoval}
              size="sm"
              variant="secondary"
            >
              Remove Project
            </Button>
            <Button
              disabled={isProjectMutationPending}
              leadingIcon="refresh"
              onClick={handleReloadProject}
              size="sm"
              variant="secondary"
            >
              Reload
            </Button>
            <Button
              disabled={
                !hasPendingChanges ||
                isProjectMutationPending ||
                isEditingLocked
              }
              onClick={() => void handleSaveProject()}
              size="sm"
              variant="primary"
            >
              {isSaving ? "Saving..." : "Save Changes"}
            </Button>
          </div>
        }
        description={repository.repo_url}
        eyebrow="Repository Project"
        summary={
          <MetaRow>
            <MetaItem label="Draft">
              {hasPendingChanges ? "Unsaved changes" : "Saved"}
            </MetaItem>
            <MetaItem label="Status">{draft?.enabled ?? "enabled"}</MetaItem>
            <MetaItem label="Cadence">{`${pollingIntervalLabel}s`}</MetaItem>
            <MetaItem label="Targets">
              {formatTargetCount(activeTargetCount)}
            </MetaItem>
            <MetaItem label="Access">
              {formatRepositoryAccessSummary(
                repositoryAccessAssessment,
                isAssessingRepositoryAccess,
                repositoryAccessError,
              )}
            </MetaItem>
            <MetaItem label="Connection">
              {formatRepositoryBindingSummary(
                repository,
                desiredRepositoryCredentialId,
              )}
            </MetaItem>
          </MetaRow>
        }
        title={draft?.name.trim() || repository.repository_name}
      >
        {saveMessage ? <p className="notice-banner">{saveMessage}</p> : null}
        {saveError ? (
          <p className="feed-banner feed-banner--error">{saveError}</p>
        ) : null}
        {isEditingLocked ? (
          <p className="feed-banner">
            Project changes are available only when no related processes are
            running.
          </p>
        ) : null}

        <div className="project-detail-stage-shell">
          <ProjectDetailSectionTabs
            activeSection={activeSection}
            disabled={isEditingLocked}
            onChange={handleSectionTabChange}
          />

          <fieldset
            className="project-detail-edit-lock-shell"
            disabled={isEditingLocked}
          >
            <div className="project-detail-stage-shell__content">
              <ProjectDetailSectionPanel
                description="Edit project-scoped settings that do not belong to the repository definition."
                eyebrow="Project Settings"
                open={sectionOpenState.project}
                sectionKey="project"
                summary={
                  <MetaRow>
                    <MetaItem label="Status">
                      {draft?.enabled ?? "enabled"}
                    </MetaItem>
                    <MetaItem label="Engine">
                      {draft?.engineKind ?? "unity"}
                    </MetaItem>
                    <MetaItem label="Name">
                      {draft?.name.trim() || repository.repository_name}
                    </MetaItem>
                  </MetaRow>
                }
                title="Project Settings"
              >
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

                      <RepositoryEngineField
                        error={validationErrors.engineKind}
                        onChange={(event) =>
                          handleDraftFieldChange(
                            "engineKind",
                            event.target
                              .value as RepositoryProjectDraft["engineKind"],
                          )
                        }
                        value={draft.engineKind}
                      />

                      <SelectField
                        label="Project status"
                        onChange={(event) =>
                          handleDraftFieldChange(
                            "enabled",
                            event.target
                              .value as RepositoryProjectDraft["enabled"],
                          )
                        }
                        options={PROJECT_STATUS_OPTIONS}
                        value={draft.enabled}
                      />
                    </div>
                  </div>
                ) : null}
              </ProjectDetailSectionPanel>

              <ProjectDetailSectionPanel
                description="Configure the remote, branch, polling cadence, visibility, and repository authentication."
                eyebrow="Repository"
                open={sectionOpenState.repository}
                sectionKey="repository"
                summary={
                  <MetaRow>
                    <MetaItem label="Branch">
                      {formatOptionalBranchLabel(draft?.defaultBranch ?? "")}
                    </MetaItem>
                    <MetaItem label="Visibility">
                      {draft?.repositoryVisibility ?? "public"}
                    </MetaItem>
                    <MetaItem label="Access">
                      {formatRepositoryAccessSummary(
                        repositoryAccessAssessment,
                        isAssessingRepositoryAccess,
                        repositoryAccessError,
                      )}
                    </MetaItem>
                  </MetaRow>
                }
                title="Repository"
              >
                {draft ? (
                  <div className="project-detail-form">
                    <div className="project-detail-form-grid">
                      <TextField
                        error={validationErrors.repositoryUrl}
                        label="Repository URL"
                        onChange={(event) =>
                          handleDraftFieldChange(
                            "repositoryUrl",
                            event.target.value,
                          )
                        }
                        placeholder="https://example.com/repository.git"
                        value={draft.repositoryUrl}
                      />

                      <TextField
                        hint="Optional"
                        label="Default branch"
                        onChange={(event) =>
                          handleDraftFieldChange(
                            "defaultBranch",
                            event.target.value,
                          )
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
                        hint="Tell HGP whether this remote should be treated as public or private."
                        label="Repository visibility"
                        onChange={(event) =>
                          handleDraftFieldChange(
                            "repositoryVisibility",
                            event.target.value,
                          )
                        }
                        options={REPOSITORY_VISIBILITY_OPTIONS}
                        value={draft.repositoryVisibility}
                      />

                      <div className="project-detail-form-grid__span-full">
                        <div className="wizard-callout wizard-callout--compact wizard-callout--auth wizard-callout--support">
                          <div className="wizard-callout__header">
                            <div>
                              <p className="wizard-callout__title">
                                Repository access
                              </p>
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
                            <SummaryStrip className="wizard-callout__summary-strip">
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
                                    desiredRepositoryCredentialId,
                                    pendingRepositoryAccessAction,
                                  )}
                                </MetaItem>
                              </MetaRow>
                            </SummaryStrip>
                          ) : null}

                          {repositoryAccessActionMessage ? (
                            <p className="notice-banner">
                              {repositoryAccessActionMessage}
                            </p>
                          ) : null}

                          {validationErrors.repositoryAccess ? (
                            <p className="ui-field__error">
                              {validationErrors.repositoryAccess}
                            </p>
                          ) : null}

                          {shouldShowRepositoryLoginAction(
                            repositoryAccessAssessment,
                          ) ? (
                            <>
                              <SelectField
                                disabled={
                                  isLoadingRepositoryCredentials ||
                                  pendingRepositoryAccessAction
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
                                value={
                                  desiredRepositoryCredentialId?.toString() ??
                                  ""
                                }
                              />

                              <div className="wizard-callout__actions">
                                {supportsShellRepositoryLoginAction(
                                  repositoryAccessAssessment,
                                ) ? (
                                  <Button
                                    disabled={pendingRepositoryAccessAction}
                                    leadingIcon="key"
                                    onClick={() =>
                                      void handleBindRepositoryAccess()
                                    }
                                    size="sm"
                                    variant={
                                      desiredRepositoryCredentialId !== null
                                        ? "secondary"
                                        : "primary"
                                    }
                                  >
                                    {pendingRepositoryAccessAction
                                      ? "Connecting login..."
                                      : formatRepositoryBindingActionLabel(
                                          repositoryAccessAssessment,
                                          githubAuthProvider,
                                          desiredRepositoryCredentialId,
                                        )}
                                  </Button>
                                ) : null}

                                {desiredRepositoryCredentialId !== null ? (
                                  <Button
                                    onClick={handleClearRepositoryAccessBinding}
                                    size="sm"
                                    variant="ghost"
                                  >
                                    Disconnect
                                  </Button>
                                ) : null}
                              </div>
                            </>
                          ) : null}
                        </div>
                      </div>
                    </div>
                  </div>
                ) : null}
              </ProjectDetailSectionPanel>

              <ProjectDetailSectionPanel
                description="Choose optional repository-specific artifact and workspace paths."
                eyebrow="Paths"
                open={sectionOpenState.paths}
                sectionKey="paths"
                summary={
                  <MetaRow>
                    <MetaItem label="Artifacts">
                      {formatPathOverrideState(
                        draft?.artifactsRootOverride ?? "",
                      )}
                    </MetaItem>
                    <MetaItem label="Workspace">
                      {formatPathOverrideState(
                        draft?.workspaceRootOverride ?? "",
                      )}
                    </MetaItem>
                  </MetaRow>
                }
                title="Paths"
              >
                {draft ? (
                  <div className="project-detail-form">
                    <div className="project-detail-form-grid">
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
                            setSaveError(
                              buildProjectSaveErrorMessage(pickError),
                            );
                          }}
                          onPathPicked={(path) =>
                            handleDraftFieldChange(
                              "artifactsRootOverride",
                              path,
                            )
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
                            setSaveError(
                              buildProjectSaveErrorMessage(pickError),
                            );
                          }}
                          onPathPicked={(path) =>
                            handleDraftFieldChange(
                              "workspaceRootOverride",
                              path,
                            )
                          }
                          pickerKind="directory"
                          placeholder="Uses the runtime workspace root when empty"
                          value={draft.workspaceRootOverride}
                        />
                      </div>
                    </div>
                  </div>
                ) : null}
              </ProjectDetailSectionPanel>

              <ProjectDetailSectionPanel
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
                open={sectionOpenState.targets}
                sectionKey="targets"
                summary={
                  <MetaRow>
                    <MetaItem label="Targets">
                      {formatTargetCount(activeTargetCount)}
                    </MetaItem>
                    <MetaItem label="Diagnostics">
                      {formatTargetAttentionSummary(targetAttentionCount)}
                    </MetaItem>
                    {validatingTargetCount > 0 ? (
                      <MetaItem label="Validation">
                        {`${validatingTargetCount} running`}
                      </MetaItem>
                    ) : null}
                  </MetaRow>
                }
                title="Build Targets"
              >
                {validationErrors.buildTargetsRoot ? (
                  <p className="feed-banner feed-banner--error">
                    {validationErrors.buildTargetsRoot}
                  </p>
                ) : null}

                {pendingBuildTargetRemoval ? (
                  <BuildTargetRemovalCallout
                    bindingImpact={pendingBuildTargetBindingImpact}
                    cancelDisabled={isSaving}
                    confirmDisabled={isSaving}
                    onCancel={() => setPendingBuildTargetRemovalId(null)}
                    onConfirm={handleConfirmBuildTargetRemoval}
                    targetName={pendingBuildTargetRemoval.name}
                  />
                ) : null}

                {draft && draft.buildTargets.length === 0 ? (
                  <div className="feed-state">
                    <p className="feed-state__title">
                      No build targets configured.
                    </p>
                    <p className="feed-state__copy">
                      This repository will not produce build work until at least
                      one target is enabled.
                    </p>
                  </div>
                ) : (
                  <div className="project-detail-target-list">
                    {draft?.buildTargets.map((target, index) => {
                      const diagnostics = pathDiagnostics[target.id];
                      const fieldErrors =
                        validationErrors.buildTargets[target.id] ?? {};
                      const bindingDestinations =
                        collectBuildTargetBindingImpact(
                          draft.publishDestinations,
                          target.id,
                        );
                      const isOpen = Boolean(expandedTargetIds[target.id]);

                      return (
                        <FormSection
                          key={target.id}
                          title={
                            target.name.trim() || `Build target ${index + 1}`
                          }
                          description={
                            target.targetPlatform.trim() || "no Unity target"
                          }
                          actions={
                            <div
                              style={{
                                display: "flex",
                                gap: 8,
                                alignItems: "center",
                              }}
                            >
                              <IconButton
                                disabled={
                                  draft.buildTargets.length === 1 || isSaving
                                }
                                icon="trash"
                                label={`Remove build target ${index + 1}`}
                                onClick={() =>
                                  handleRemoveBuildTarget(target.id)
                                }
                                size="sm"
                                variant="ghost"
                              />
                              <Button
                                onClick={() =>
                                  handleTargetAccordionChange(
                                    target.id,
                                    !isOpen,
                                  )
                                }
                                size="sm"
                                variant="ghost"
                              >
                                {isOpen ? "Collapse" : "Edit"}
                              </Button>
                            </div>
                          }
                          summary={
                            isOpen ? null : (
                              <ProjectDetailBuildTargetSummary
                                bindingDestinations={bindingDestinations}
                                diagnostics={diagnostics}
                                isValidating={Boolean(
                                  validatingTargets[target.id],
                                )}
                                target={target}
                              />
                            )
                          }
                        >
                          {isOpen ? (
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
                                    targetPlatform:
                                      normalizeUnityTargetPlatformValue(
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
                                  setSaveError(
                                    buildProjectSaveErrorMessage(pickError),
                                  );
                                }}
                                onPathPicked={(selectedPath) =>
                                  handlePickUnityExecutablePath(
                                    target.id,
                                    selectedPath,
                                  )
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
                          ) : null}
                        </FormSection>
                      );
                    })}
                  </div>
                )}
              </ProjectDetailSectionPanel>

              <ProjectDetailSectionPanel
                description="Edit publish destinations, credentials, and per-target binding semantics."
                eyebrow="Publishing"
                open={sectionOpenState.destinations}
                sectionKey="destinations"
                summary={
                  <MetaRow>
                    <MetaItem label="Destinations">
                      {formatPublishDestinationCount(publishDestinationCount)}
                    </MetaItem>
                    <MetaItem label="Non-consuming">
                      {enabledNonConsumingBindingCount}
                    </MetaItem>
                    <MetaItem label="Consuming">
                      {enabledConsumingBindingCount}
                    </MetaItem>
                  </MetaRow>
                }
                title="Publish Destinations"
              >
                <div className="project-detail-target-list">
                  <PublishDestinationsEditor
                    buildTargets={buildTargetReferences}
                    credentials={publishCredentials}
                    destinations={draft.publishDestinations}
                    disabled={isSaving}
                    errors={validationErrors.publishDestinations}
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
                  />

                  <ProjectDetailPublishSupportPanel
                    publishDestinationReviewSummary={
                      publishDestinationReviewSummary
                    }
                    unboundPublishTargetNames={unboundPublishTargetNames}
                  />
                </div>
              </ProjectDetailSectionPanel>

              <ProjectDetailSectionPanel
                description="Queue and execution backlog for the registered repository."
                eyebrow="Automation"
                open={sectionOpenState.automation}
                sectionKey="automation"
                summary={
                  <MetaRow>
                    <MetaItem label="Pending">
                      {repository.pending_release_count}
                    </MetaItem>
                    <MetaItem label="Queued">
                      {repository.queued_build_runs}
                    </MetaItem>
                    <MetaItem label="Running">{runningWorkCount}</MetaItem>
                  </MetaRow>
                }
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
              </ProjectDetailSectionPanel>
            </div>
          </fieldset>
        </div>
        {isProjectRemovalOpen ? (
          <ProjectRemovalDialog
            hasPendingChanges={hasPendingChanges}
            isRemoving={isRemovingProject}
            onCancel={handleCloseProjectRemoval}
            onRemove={handleRemoveProject}
            projectName={draft?.name.trim() || repository.repository_name}
            removalError={projectRemovalError}
          />
        ) : null}
      </FocusPageFrame>
    </div>
  );
}

function ProjectDetailSectionTabs({
  activeSection,
  disabled = false,
  onChange,
}: {
  activeSection: ProjectDetailSectionKey;
  disabled?: boolean;
  onChange: (sectionKey: ProjectDetailSectionKey) => void;
}) {
  const tabRefs = useRef<
    Partial<Record<ProjectDetailSectionKey, HTMLButtonElement | null>>
  >({});

  const handleTabKeyDown =
    (key: ProjectDetailSectionKey) =>
    (event: React.KeyboardEvent<HTMLButtonElement>) => {
      if (disabled) {
        return;
      }

      const currentIndex = PROJECT_DETAIL_SECTION_TABS.findIndex(
        (tab) => tab.key === key,
      );

      if (currentIndex < 0) {
        return;
      }

      let nextIndex: number | null = null;

      switch (event.key) {
        case "ArrowUp":
          nextIndex =
            currentIndex === 0
              ? PROJECT_DETAIL_SECTION_TABS.length - 1
              : currentIndex - 1;
          break;
        case "ArrowDown":
          nextIndex =
            currentIndex === PROJECT_DETAIL_SECTION_TABS.length - 1
              ? 0
              : currentIndex + 1;
          break;
        case "Home":
          nextIndex = 0;
          break;
        case "End":
          nextIndex = PROJECT_DETAIL_SECTION_TABS.length - 1;
          break;
        default:
          return;
      }

      event.preventDefault();

      const nextTab = PROJECT_DETAIL_SECTION_TABS[nextIndex];

      onChange(nextTab.key);
      requestAnimationFrame(() => {
        tabRefs.current[nextTab.key]?.focus();
      });
    };

  return (
    <div
      aria-label="Project detail sections"
      aria-orientation="vertical"
      className="project-detail-tablist ui-panel ui-panel--inset"
      role="tablist"
    >
      {PROJECT_DETAIL_SECTION_TABS.map((tab) => {
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

function ProjectDetailSectionPanel({
  actions,
  children,
  description,
  eyebrow,
  open,
  sectionKey,
  summary,
  title,
}: {
  actions?: ReactNode;
  children: ReactNode;
  description: string;
  eyebrow: string;
  open: boolean;
  sectionKey: ProjectDetailSectionKey;
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
            <p className="ui-panel__eyebrow">{eyebrow}</p>
            <h2 className="ui-panel__title">{title}</h2>
            <p className="ui-panel__description">{description}</p>
            {summary ? (
              <SummaryStrip className="project-detail-section-accordion__summary">
                {summary}
              </SummaryStrip>
            ) : null}
          </div>
          {actions ? (
            <div className="project-detail-section-accordion__actions">
              {actions}
            </div>
          ) : null}
        </div>
      </div>
      <div className="ui-panel__body project-detail-section-panel__body">
        {children}
      </div>
    </section>
  );
}

function ProjectDetailBuildTargetSummary({
  bindingDestinations,
  diagnostics,
  isValidating,
  target,
}: {
  bindingDestinations: string[];
  diagnostics: UnityExecutableValidation | null;
  isValidating: boolean;
  target: RepositoryProjectBuildTargetDraft;
}) {
  return (
    <div className="project-detail-target-card">
      <SummaryStrip className="project-detail-target-card__summary-strip">
        <MetaRow className="wizard-target-card__summary">
          <MetaItem label="Build method">
            {formatBuildTargetMethodSummary(target.buildMethod)}
          </MetaItem>
          <MetaItem label="Unity executable">
            {formatCollapsedBuildTargetUnitySummary(
              diagnostics,
              isValidating,
              target.unityExecutablePath,
            )}
          </MetaItem>
          <MetaItem label="Publish bindings">
            {formatCollapsedBuildTargetBindingSummary(bindingDestinations)}
          </MetaItem>
        </MetaRow>
      </SummaryStrip>
    </div>
  );
}

function ProjectDetailPublishSupportPanel({
  publishDestinationReviewSummary,
  unboundPublishTargetNames,
}: {
  publishDestinationReviewSummary: ReturnType<
    typeof buildPublishDestinationReviewSummary
  >;
  unboundPublishTargetNames: string[];
}) {
  const missingCredentialCount = publishDestinationReviewSummary.filter(
    (destination) => destination.missingCredential,
  ).length;
  const draftImpactSummary = publishDestinationReviewSummary.length
    ? publishDestinationReviewSummary
        .map((destination) => {
          const targetSummary = destination.bindingTargetNames.length
            ? destination.bindingTargetNames.join(", ")
            : "no bound targets";
          return `${destination.name}: ${targetSummary}`;
        })
        .join(" | ")
    : "No publish destinations are currently configured.";

  return (
    <SurfacePanel
      description="Unbound targets stay local under the runtime-managed output root until a destination binding consumes or uploads them."
      eyebrow="Support"
      title="Draft impact"
      tone="inset"
    >
      <MetaRow>
        <MetaItem label="Unbound targets">
          {String(unboundPublishTargetNames.length)}
        </MetaItem>
        <MetaItem label="Credential gaps">{missingCredentialCount}</MetaItem>
      </MetaRow>
      <p className="project-detail-target-card__copy">{draftImpactSummary}</p>
    </SurfacePanel>
  );
}

function formatRepositoryAccessSummary(
  assessment: RepositoryAccessAssessment | null,
  isAssessingRepositoryAccess: boolean,
  repositoryAccessError: string | null,
) {
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

function formatRepositoryBindingSummary(
  repository: RepositoryInspectionEntry,
  repositoryCredentialId: number | null,
) {
  const persistedCredentialId = repository.credentials?.credential_id ?? null;

  if (
    repositoryCredentialId !== null &&
    repositoryCredentialId !== persistedCredentialId
  ) {
    return "Pending save";
  }

  if (repositoryCredentialId !== null) {
    return repository.credentials?.name ?? "Bound";
  }

  return "No credential";
}

function formatOptionalBranchLabel(branch: string) {
  const trimmed = branch.trim();
  return trimmed ? trimmed : "No default branch";
}

function formatTargetCount(targetCount: number) {
  return `${targetCount} active target${targetCount === 1 ? "" : "s"}`;
}

function formatTargetAttentionSummary(targetAttentionCount: number) {
  if (targetAttentionCount === 0) {
    return "All ready";
  }

  return `${targetAttentionCount} need review`;
}

function formatBuildTargetMethodSummary(buildMethod: string) {
  const trimmed = buildMethod.trim();
  return trimmed ? trimmed : "Missing build method";
}

function formatCollapsedBuildTargetUnitySummary(
  diagnostics: UnityExecutableValidation | null,
  isValidating: boolean,
  unityExecutablePath: string,
) {
  if (isValidating) {
    return "Checking";
  }

  if (!unityExecutablePath.trim()) {
    return "Missing path";
  }

  if (!diagnostics) {
    return "Pending check";
  }

  return diagnostics.status === "ready" ? "Ready" : "Needs review";
}

function formatCollapsedBuildTargetBindingSummary(
  bindingDestinations: string[],
) {
  if (bindingDestinations.length === 0) {
    return "No publish bindings";
  }

  if (bindingDestinations.length <= 2) {
    return bindingDestinations.join(", ");
  }

  return `${bindingDestinations.slice(0, 2).join(", ")} +${bindingDestinations.length - 2} more`;
}

function countEnabledPublishDestinationBindings(
  destinations: PublishDestinationDraft[],
  behavior: RepositoryPublishBindingInspection["consumption_behavior"],
) {
  return destinations.reduce((total, destination) => {
    const matchesBehavior =
      (behavior === "consuming" && destination.kind === "filesystem") ||
      (behavior === "non_consuming" && destination.kind === "itch");
    if (!matchesBehavior) {
      return total;
    }

    return (
      total + destination.bindings.filter((binding) => binding.enabled).length
    );
  }, 0);
}

function formatPublishDestinationCount(destinationCount: number) {
  return `${destinationCount} destination${destinationCount === 1 ? "" : "s"}`;
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
    return "Connect login";
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

function buildProjectRemoveErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return "The desktop shell could not remove the project.";
}

type ProjectRemovalDialogProps = {
  hasPendingChanges: boolean;
  isRemoving: boolean;
  onCancel: () => void;
  onRemove: (strategy: RemoveRepositoryProjectStrategy) => void;
  projectName: string;
  removalError: string | null;
};

function ProjectRemovalDialog({
  hasPendingChanges,
  isRemoving,
  onCancel,
  onRemove,
  projectName,
  removalError,
}: ProjectRemovalDialogProps) {
  const resolvedProjectName = projectName.trim() || "this project";

  return (
    <FullScreenModal
      className="project-removal-dialog__modal"
      description="Choose whether HGP should only remove the project from SQLite or also purge runtime-owned files from disk."
      dismissible={!isRemoving}
      onResolve={() => onCancel()}
      title={`Remove ${resolvedProjectName}?`}
    >
      <div className="project-removal-dialog">
        <p className="project-removal-dialog__copy">
          {hasPendingChanges
            ? "Unsaved edits will be discarded when the project is removed."
            : "Select how thoroughly HGP should remove this project from the app."}
        </p>

        {removalError ? (
          <p className="feed-banner feed-banner--error">{removalError}</p>
        ) : null}

        <div className="project-removal-dialog__options">
          <section className="project-removal-dialog__option">
            <div>
              <h3 className="project-removal-dialog__option-title">
                Remove from App Only
              </h3>
              <p className="project-removal-dialog__option-copy">
                Deletes the project from SQLite and keeps workspaces, artifacts,
                logs, and retained files on disk.
              </p>
            </div>

            <div className="project-removal-dialog__option-actions">
              <Button
                disabled={isRemoving}
                onClick={() => onRemove("detach")}
                size="sm"
                variant="primary"
              >
                Remove from App Only
              </Button>
            </div>
          </section>

          <section className="project-removal-dialog__option">
            <div>
              <h3 className="project-removal-dialog__option-title">
                Purge Total
              </h3>
              <p className="project-removal-dialog__option-copy">
                Deletes the project from SQLite and removes runtime-owned
                workspaces, artifacts, logs, and retained files collected for
                this project.
              </p>
            </div>

            <div className="project-removal-dialog__option-actions">
              <Button
                className="project-removal-dialog__action--purge"
                disabled={isRemoving}
                onClick={() => onRemove("purge")}
                size="sm"
                variant="secondary"
              >
                Purge Total
              </Button>
            </div>
          </section>
        </div>

        <div className="confirm-dialog__actions">
          <Button
            data-overlay-autofocus
            disabled={isRemoving}
            onClick={onCancel}
            size="sm"
            variant="ghost"
          >
            Cancel
          </Button>
        </div>
      </div>
    </FullScreenModal>
  );
}

function createEmptyValidationErrors(): RepositoryProjectValidationErrors {
  return {
    buildTargets: {},
    publishDestinations: createEmptyPublishDestinationValidationErrors(),
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
    repositoryVisibility: resolveRepositoryVisibilitySelection(repository),
    defaultBranch: repository.default_branch ?? "",
    artifactsRootOverride: repository.artifacts_root_override ?? "",
    workspaceRootOverride: repository.workspace_root_override ?? "",
    pollingIntervalSeconds: String(repository.polling_interval_seconds),
    enabled: repository.enabled ? "enabled" : "disabled",
    buildTargets,
    publishDestinations: buildPublishDestinationDrafts(
      repository.publish_targets,
      buildTargets.map((target) => ({
        id: target.id,
        buildTargetId: target.buildTargetId,
        name: target.name.trim() || "Unnamed target",
      })),
    ),
  };
}

function buildRepositoryAccessAssessmentFromRepository(
  repository: RepositoryInspectionEntry,
): RepositoryAccessAssessment | null {
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
): RepositoryProjectDraft["repositoryVisibility"] {
  if (
    repository.visibility_status === "private" ||
    repository.auth_requirement_status === "required" ||
    repository.credentials !== null
  ) {
    return "private";
  }

  return "public";
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
    expandedTargetIds[targetId] = false;
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
  } else if (authState.isAssessingRepositoryAccess) {
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
      !supportsShellRepositoryLoginAction(authState.repositoryAccessAssessment)
    ) {
      const providerLabel =
        authState.repositoryAccessAssessment.provider_id === "unknown"
          ? "this host"
          : authState.repositoryAccessAssessment.provider_label || "this host";
      errors.repositoryAccess =
        authState.repositoryAccessAssessment.provider_id === "unknown"
          ? "Private repositories are not supported for this host yet. Only public repositories can be saved right now."
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
            "Private GitHub repository detected. Log in and connect a GitHub credential, or select another stored repository credential, before saving.";
        } else {
          errors.repositoryAccess =
            "Private GitHub repository detected. Connect a credential to this project before saving.";
        }
      } else if (authState.repositoryCredentialCount === 0) {
        errors.repositoryAccess = `Private ${providerLabel} repository detected. No stored repository credentials are available for this project yet.`;
      } else {
        errors.repositoryAccess = `Private ${providerLabel} repository detected. Select a stored repository credential before saving.`;
      }
    } else {
      errors.repositoryAccess = undefined;
    }
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

    errors.buildTargets[target.id] = fieldErrors;
  }

  errors.publishDestinations = validatePublishDestinationDrafts(
    draft.publishDestinations,
    draft.buildTargets.map((target) => ({
      id: target.id,
      buildTargetId: target.buildTargetId,
      name: target.name.trim() || "Unnamed target",
    })),
  );

  return errors;
}

function hasValidationErrors(errors: RepositoryProjectValidationErrors) {
  return Boolean(
    errors.engineKind ||
    errors.name ||
    errors.repositoryUrl ||
    errors.pollingIntervalSeconds ||
    errors.repositoryAccess ||
    errors.buildTargetsRoot ||
    hasPublishDestinationValidationErrors(errors.publishDestinations) ||
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

function buildProjectDetailSectionState(
  activeSection: ProjectDetailSectionKey,
): Record<ProjectDetailSectionKey, boolean> {
  return {
    project: activeSection === "project",
    repository: activeSection === "repository",
    paths: activeSection === "paths",
    targets: activeSection === "targets",
    destinations: activeSection === "destinations",
    automation: activeSection === "automation",
  };
}

function resolveActiveProjectDetailSection(
  sectionOpenState: Record<ProjectDetailSectionKey, boolean>,
): ProjectDetailSectionKey {
  return (
    PROJECT_DETAIL_SECTION_TABS.find((tab) => sectionOpenState[tab.key])?.key ??
    DEFAULT_PROJECT_DETAIL_SECTION
  );
}

function resolveInvalidProjectDetailSection(
  validationErrors: RepositoryProjectValidationErrors,
  invalidTargetIds: string[],
  hasPublishErrors: boolean,
): ProjectDetailSectionKey {
  if (validationErrors.engineKind || validationErrors.name) {
    return "project";
  }

  if (
    validationErrors.repositoryUrl ||
    validationErrors.pollingIntervalSeconds ||
    validationErrors.repositoryAccess
  ) {
    return "repository";
  }

  if (validationErrors.buildTargetsRoot || invalidTargetIds.length > 0) {
    return "targets";
  }

  if (hasPublishErrors) {
    return "destinations";
  }

  return DEFAULT_PROJECT_DETAIL_SECTION;
}

function formatPathOverrideState(value: string) {
  return value.trim() ? "Override" : "Runtime default";
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

function buildRepositoryProjectUpdateInput(
  repositoryId: number,
  draft: RepositoryProjectDraft,
  repositoryAccessAssessment: RepositoryAccessAssessment | null,
): UpdateRepositoryProjectInput {
  return {
    repository_id: repositoryId,
    name: draft.name.trim(),
    engine_kind: draft.engineKind,
    repository_url: draft.repositoryUrl.trim(),
    repository_access_assessment: repositoryAccessAssessment,
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
    publish_targets: buildUpdateProjectPublishTargetsInput(
      draft.publishDestinations,
      draft.buildTargets.map((target) => ({
        id: target.id,
        buildTargetId: target.buildTargetId,
        name: target.name.trim() || "Unnamed target",
      })),
    ),
  };
}

function buildRepositoryAccessAssessmentFromDetection(
  detection: RepositoryProviderDetection,
  repositoryVisibility: RepositoryProjectDraft["repositoryVisibility"],
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
        "Private GitHub repository selected. Log in and connect this project before saving.",
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

function areRepositoryAccessAssessmentsEqual(
  left: RepositoryAccessAssessment | null,
  right: RepositoryAccessAssessment | null,
) {
  if (left === right) {
    return true;
  }

  if (!left || !right) {
    return false;
  }

  return (
    left.provider_id === right.provider_id &&
    left.provider_label === right.provider_label &&
    left.instance_url === right.instance_url &&
    left.normalized_url === right.normalized_url &&
    left.visibility === right.visibility &&
    left.auth_requirement === right.auth_requirement &&
    left.auth_status === right.auth_status &&
    left.supports_interactive_login === right.supports_interactive_login &&
    left.message === right.message
  );
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
    !areBuildTargetDraftsEqual(
      persistedDraft.buildTargets,
      draft.buildTargets,
    ) ||
    !arePublishDestinationDraftsEqual(
      persistedDraft.publishDestinations,
      draft.publishDestinations,
    )
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
      target.unityExecutablePath.trim() === candidate.unityExecutablePath.trim()
    );
  });
}

function arePublishDestinationDraftsEqual(
  left: PublishDestinationDraft[],
  right: PublishDestinationDraft[],
) {
  if (left.length !== right.length) {
    return false;
  }

  return left.every((destination, index) => {
    const candidate = right[index];
    if (!candidate) {
      return false;
    }

    return (
      destination.publishTargetId === candidate.publishTargetId &&
      destination.name.trim() === candidate.name.trim() &&
      destination.kind === candidate.kind &&
      destination.enabled === candidate.enabled &&
      destination.itchAccountName.trim() === candidate.itchAccountName.trim() &&
      destination.itchGameSlug.trim() === candidate.itchGameSlug.trim() &&
      destination.credentialsId === candidate.credentialsId &&
      arePublishDestinationBindingDraftsEqual(
        destination.bindings,
        candidate.bindings,
      )
    );
  });
}

function arePublishDestinationBindingDraftsEqual(
  left: PublishDestinationDraft["bindings"],
  right: PublishDestinationDraft["bindings"],
) {
  if (left.length !== right.length) {
    return false;
  }

  return left.every((binding, index) => {
    const candidate = right[index];
    if (!candidate) {
      return false;
    }

    return (
      binding.buildTargetId === candidate.buildTargetId &&
      binding.buildTargetDraftId === candidate.buildTargetDraftId &&
      binding.buildTargetName.trim() === candidate.buildTargetName.trim() &&
      binding.enabled === candidate.enabled &&
      binding.filesystemDirectoryPath.trim() ===
        candidate.filesystemDirectoryPath.trim() &&
      binding.itchChannel.trim() === candidate.itchChannel.trim() &&
      binding.itchUserversionTemplate.trim() ===
        candidate.itchUserversionTemplate.trim()
    );
  });
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
