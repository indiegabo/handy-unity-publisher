import {
  startTransition,
  type ReactNode,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";

import { Button, IconButton } from "./Button";
import { RepositoryCredentialComposer } from "./RepositoryCredentialComposer";
import { SelectField, TextField, type SelectOption } from "./Field";
import { PathPickerField } from "./PathPickerField";
import { RepositoryEngineField } from "./RepositoryEngineField";
import { Badge, FocusPageFrame, MetaItem, MetaRow } from "./Surface";
import { VerticalAccordion } from "./VerticalAccordion";
import {
  connectRepositoryAuth,
  detectRepositoryProvider,
  disconnectRepositoryAuth,
  loadSecretSettings,
  loadRepositoryInspection,
  reconnectRepositoryAuth,
  saveSecretCredential,
  updateRepositoryProject,
  validateUnityExecutablePath,
  type RepositoryAccessAssessment,
  type RepositoryEngineKind,
  type RepositoryPublishBindingInspection,
  type RepositoryPublishTargetInspection,
  type RepositoryInspectionEntry,
  type RepositoryProviderDetection,
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
};

type RepositoryProjectFieldName = Exclude<
  keyof RepositoryProjectDraft,
  "buildTargets"
>;

type ProjectDetailSectionKey =
  | "project"
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
const DEFAULT_SECTION_OPEN_STATE: Record<ProjectDetailSectionKey, boolean> = {
  project: false,
  targets: false,
  destinations: false,
  automation: false,
};

export function RepositoryProjectDetail({
  onProjectNameResolved,
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
  const [isLoadingAuthProviders, setIsLoadingAuthProviders] = useState(true);
  const [authProviderError, setAuthProviderError] = useState<string | null>(
    null,
  );
  const [repositoryCredentials, setRepositoryCredentials] = useState<
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
  const [
    showRepositoryCredentialComposer,
    setShowRepositoryCredentialComposer,
  ] = useState(false);
  const [pendingRepositoryCredentialSave, setPendingRepositoryCredentialSave] =
    useState(false);
  const [repositoryCredentialSaveError, setRepositoryCredentialSaveError] =
    useState<string | null>(null);
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
  const accessAssessmentTimerRef = useRef<number | undefined>(undefined);
  const accessAssessmentTokenRef = useRef(0);

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
    return settings.credentials.filter(isRepositoryCredentialSelectable);
  });

  const loadRepositoryCredentialsEffect = useEffectEvent(async () => {
    setIsLoadingRepositoryCredentials(true);

    try {
      const credentials = await listRepositoryCredentialsEffect();

      startTransition(() => {
        setRepositoryCredentials(credentials);
        setRepositoryCredentialsError(null);
        setIsLoadingRepositoryCredentials(false);
      });
    } catch (loadError) {
      startTransition(() => {
        setRepositoryCredentials([]);
        setRepositoryCredentialsError(buildProjectSaveErrorMessage(loadError));
        setIsLoadingRepositoryCredentials(false);
      });
    }
  });

  useEffect(() => {
    void loadAuthProvidersEffect();
    void loadRepositoryCredentialsEffect();
  }, [loadAuthProvidersEffect, loadRepositoryCredentialsEffect]);

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
    const repositoryUrl = draft?.repositoryUrl ?? "";
    const repositoryVisibility = draft?.repositoryVisibility ?? "public";
    const normalizedUrl = repositoryUrl.trim();

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
    draft?.repositoryUrl,
    draft?.repositoryVisibility,
    loadRepositoryAccessAssessmentEffect,
  ]);

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
          "GitHub login connected for this project. Save changes to keep the connection.",
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
        setRepositoryCredentialSaveError(null);
        setShowRepositoryCredentialComposer(false);
        setSaveError(null);
      });
    },
  );

  const handleOpenRepositoryCredentialComposer = useEffectEvent(() => {
    startTransition(() => {
      setShowRepositoryCredentialComposer(true);
      setRepositoryCredentialSaveError(null);
      setSaveError(null);
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
        setSaveError(null);
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
          setRepositoryCredentials(credentials);
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
          setRepositoryCredentialSaveError(buildProjectSaveErrorMessage(error));
        });
      } finally {
        startTransition(() => {
          setPendingRepositoryCredentialSave(false);
        });
      }
    },
  );

  const handleSaveProject = useEffectEvent(async () => {
    if (!repository || !draft || isSaving) {
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
            Boolean(
              nextValidationErrors.buildTargetsRoot || invalidTargetIds.length,
            ),
        }));
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
          <p className="feed-banner feed-banner--error">{error}</p>
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
  const targetAttentionCount = draft
    ? draft.buildTargets.filter((target) => {
        const diagnostics = pathDiagnostics[target.id];

        return diagnostics !== null && diagnostics.status !== "ready";
      }).length
    : 0;
  const publishDestinationCount = repository.publish_targets.length;
  const enabledNonConsumingBindingCount = countEnabledPublishBindings(
    repository,
    "non_consuming",
  );
  const enabledConsumingBindingCount = countEnabledPublishBindings(
    repository,
    "consuming",
  );
  const pollingIntervalLabel =
    draft?.pollingIntervalSeconds.trim() ||
    String(repository.polling_interval_seconds);
  const runningWorkCount =
    repository.running_build_runs + repository.running_publish_runs;

  return (
    <div className="project-detail-shell">
      <FocusPageFrame
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
              disabled={!hasPendingChanges || isSaving}
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

        <ProjectDetailSectionAccordion
          description="Edit the repository identity, cadence, and runtime-managed paths."
          eyebrow="Repository Settings"
          onOpenChange={(nextOpen) =>
            handleSectionOpenChange("project", nextOpen)
          }
          open={sectionOpenState.project}
          summary={
            <MetaRow>
              <MetaItem label="Status">{draft?.enabled ?? "enabled"}</MetaItem>
              <MetaItem label="Engine">{draft?.engineKind ?? "unity"}</MetaItem>
              <MetaItem label="Branch">
                {formatOptionalBranchLabel(draft?.defaultBranch ?? "")}
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
                      event.target
                        .value as RepositoryProjectDraft["engineKind"],
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
                          value={
                            desiredRepositoryCredentialId?.toString() ?? ""
                          }
                        />

                        <div className="wizard-callout__actions">
                          {supportsShellRepositoryLoginAction(
                            repositoryAccessAssessment,
                          ) ? (
                            <Button
                              disabled={
                                pendingRepositoryAccessAction ||
                                pendingRepositoryCredentialSave
                              }
                              leadingIcon="key"
                              onClick={() => void handleBindRepositoryAccess()}
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
                        </div>

                        {showRepositoryCredentialComposer &&
                        repositoryAccessAssessment ? (
                          <RepositoryCredentialComposer
                            isSaving={pendingRepositoryCredentialSave}
                            onCancel={handleCloseRepositoryCredentialComposer}
                            onSave={handleSaveRepositoryCredential}
                            providerLabel={
                              repositoryAccessAssessment.provider_label
                            }
                            saveError={repositoryCredentialSaveError}
                          />
                        ) : null}
                      </>
                    ) : null}
                  </div>
                </div>

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
          onOpenChange={(nextOpen) =>
            handleSectionOpenChange("targets", nextOpen)
          }
          open={sectionOpenState.targets}
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
                const fieldErrors =
                  validationErrors.buildTargets[target.id] ?? {};

                return (
                  <VerticalAccordion
                    bodyClassName="wizard-target-card__body"
                    bodyInset
                    className="wizard-target-card project-detail-target-accordion"
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
                            disabled={
                              draft.buildTargets.length === 1 || isSaving
                            }
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
          description="Inspect publish destinations, bound credentials, and per-target binding semantics."
          eyebrow="Publishing"
          onOpenChange={(nextOpen) =>
            handleSectionOpenChange("destinations", nextOpen)
          }
          open={sectionOpenState.destinations}
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
          {repository.publish_targets.length === 0 ? (
            <div className="feed-state">
              <p className="feed-state__title">
                No publish destinations configured.
              </p>
              <p className="feed-state__copy">
                This repository can build artifacts, but no publish destination
                has been registered for inspection yet.
              </p>
            </div>
          ) : (
            <div className="project-detail-target-list">
              <div className="wizard-callout wizard-callout--compact wizard-callout--support">
                <div className="wizard-callout__header">
                  <div>
                    <p className="wizard-callout__title">
                      Binding dispatch rules
                    </p>
                    <p className="wizard-callout__copy">
                      Non-consuming bindings always run before the single
                      consuming binding on the same build target. HGP rejects
                      more than one enabled consuming binding for that target.
                    </p>
                  </div>

                  <div className="wizard-callout__badges">
                    <Badge tone="neutral">ordered by semantics</Badge>
                  </div>
                </div>
              </div>

              {repository.publish_targets.map((publishTarget) => {
                const enabledBindingCount = publishTarget.bindings.filter(
                  (binding) => binding.enabled,
                ).length;
                const consumingBindingCount = publishTarget.bindings.filter(
                  (binding) =>
                    binding.enabled &&
                    binding.consumption_behavior === "consuming",
                ).length;

                return (
                  <div
                    className="project-detail-target-card"
                    key={publishTarget.publish_target_id}
                  >
                    <div className="project-detail-target-card__header">
                      <div className="project-detail-target-card__title-block">
                        <h3 className="project-detail-target-card__title">
                          {publishTarget.name}
                        </h3>
                        <p className="project-detail-target-card__copy">
                          {formatPublishTargetConfigSummary(publishTarget)}
                        </p>
                      </div>

                      <div className="project-detail-target-card__badges">
                        <Badge
                          tone={publishTarget.enabled ? "strong" : "muted"}
                        >
                          {publishTarget.enabled ? "enabled" : "disabled"}
                        </Badge>
                        <Badge tone="neutral">
                          {formatPublishTargetKindLabel(publishTarget.kind)}
                        </Badge>
                      </div>
                    </div>

                    <MetaRow>
                      <MetaItem label="Credential">
                        {formatPublishTargetCredentialSummary(publishTarget)}
                      </MetaItem>
                      <MetaItem label="Active bindings">
                        {enabledBindingCount}
                      </MetaItem>
                      <MetaItem label="Consumers">
                        {consumingBindingCount}
                      </MetaItem>
                    </MetaRow>

                    {publishTarget.bindings.length === 0 ? (
                      <p className="project-detail-target-card__copy project-detail-target-card__copy--muted">
                        No build target binding references this destination yet.
                      </p>
                    ) : (
                      <div className="project-detail-status-grid">
                        {publishTarget.bindings.map((binding) => (
                          <div
                            className="project-detail-target-card"
                            key={`${publishTarget.publish_target_id}-${binding.build_target_id}`}
                          >
                            <div className="project-detail-target-card__header">
                              <div className="project-detail-target-card__title-block">
                                <h4 className="project-detail-target-card__title">
                                  {binding.build_target_name}
                                </h4>
                                <p className="project-detail-target-card__copy">
                                  {formatPublishBindingSemanticsCopy(binding)}
                                </p>
                              </div>

                              <div className="project-detail-target-card__badges">
                                <Badge
                                  tone={binding.enabled ? "strong" : "muted"}
                                >
                                  {binding.enabled ? "enabled" : "disabled"}
                                </Badge>
                                <Badge
                                  tone={
                                    binding.consumption_behavior === "consuming"
                                      ? "neutral"
                                      : "strong"
                                  }
                                >
                                  {formatPublishBindingBehaviorLabel(
                                    binding.consumption_behavior,
                                  )}
                                </Badge>
                              </div>
                            </div>

                            <p className="project-detail-target-card__copy project-detail-target-card__copy--muted">
                              {formatPublishBindingOptionsSummary(
                                publishTarget.kind,
                                binding.options_json,
                              )}
                            </p>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
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
          summary={
            <MetaRow>
              <MetaItem label="Pending">
                {repository.pending_release_count}
              </MetaItem>
              <MetaItem label="Queued">{repository.queued_build_runs}</MetaItem>
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
        </ProjectDetailSectionAccordion>
      </FocusPageFrame>
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
  summary,
  title,
}: {
  actions?: ReactNode;
  children: ReactNode;
  description: string;
  eyebrow: string;
  onOpenChange: (nextOpen: boolean) => void;
  open: boolean;
  summary?: ReactNode;
  title: string;
}) {
  return (
    <VerticalAccordion
      bodyClassName="ui-panel__body project-detail-section-accordion__body"
      className="ui-panel ui-panel--section project-detail-section-accordion"
      collapsedToggleLabel={`Expand ${title}`}
      expandedToggleLabel={`Collapse ${title}`}
      header={
        <div className="project-detail-section-accordion__header-content">
          <div className="ui-panel__title-block">
            <p className="ui-panel__eyebrow">{eyebrow}</p>
            <h2 className="ui-panel__title">{title}</h2>
            <p className="ui-panel__description">{description}</p>
            {summary ? (
              <div className="project-detail-section-accordion__summary">
                {summary}
              </div>
            ) : null}
          </div>
          {actions ? (
            <div className="project-detail-section-accordion__actions">
              {actions}
            </div>
          ) : null}
        </div>
      }
      headerSeparated
      headerClassName="project-detail-section-accordion__header"
      onOpenChange={onOpenChange}
      open={open}
      tone="section"
      triggerMode="button"
    >
      {children}
    </VerticalAccordion>
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

function countEnabledPublishBindings(
  repository: RepositoryInspectionEntry,
  behavior: RepositoryPublishBindingInspection["consumption_behavior"],
) {
  return repository.publish_targets.reduce((total, target) => {
    return (
      total +
      target.bindings.filter(
        (binding) =>
          binding.enabled && binding.consumption_behavior === behavior,
      ).length
    );
  }, 0);
}

function formatPublishDestinationCount(destinationCount: number) {
  return `${destinationCount} destination${destinationCount === 1 ? "" : "s"}`;
}

function formatPublishTargetKindLabel(kind: string) {
  switch (kind.trim().toLocaleLowerCase()) {
    case "filesystem":
      return "filesystem";
    case "itch":
      return "itch.io";
    default:
      return kind;
  }
}

function formatPublishTargetCredentialSummary(
  publishTarget: RepositoryPublishTargetInspection,
) {
  if (!publishTarget.credentials) {
    return publishTarget.kind === "itch"
      ? "Credential missing"
      : "No credential bound";
  }

  if (publishTarget.credentials.config_status === "ready") {
    return publishTarget.credentials.name;
  }

  return `${publishTarget.credentials.name} (${formatDiagnosticStatus(
    publishTarget.credentials.config_status,
  )})`;
}

function formatPublishTargetConfigSummary(
  publishTarget: RepositoryPublishTargetInspection,
) {
  const config = parseJsonObject(publishTarget.config_json);

  if (publishTarget.kind === "filesystem") {
    const rootPath = readJsonStringField(config, "root_path");

    return rootPath
      ? `Publishes into ${rootPath}.`
      : "Publishes to a filesystem path resolved by the destination or binding.";
  }

  if (publishTarget.kind === "itch") {
    const accountName = readJsonStringField(config, "account_name");
    const gameSlug = readJsonStringField(config, "game_slug");

    if (accountName && gameSlug) {
      return `Uploads to ${accountName}/${gameSlug} through butler.`;
    }

    return "Uploads build artifacts to Itch.io through butler.";
  }

  return "Uses the persisted publish destination contract.";
}

function formatPublishBindingBehaviorLabel(
  behavior: RepositoryPublishBindingInspection["consumption_behavior"],
) {
  return behavior === "consuming" ? "consuming" : "non-consuming";
}

function formatPublishBindingSemanticsCopy(
  binding: RepositoryPublishBindingInspection,
) {
  if (binding.consumption_behavior === "consuming") {
    return binding.enabled
      ? "Runs after non-consuming bindings and becomes the artifact's active location."
      : "Disabled consuming binding. When enabled, it will run after non-consuming bindings.";
  }

  return binding.enabled
    ? "Runs before any consuming binding and leaves the artifact available for later publishes."
    : "Disabled non-consuming binding. When enabled, it will run before any consuming binding.";
}

function formatPublishBindingOptionsSummary(
  publishTargetKind: string,
  optionsJson: string,
) {
  const options = parseJsonObject(optionsJson);

  if (publishTargetKind === "filesystem") {
    const operation = readJsonStringField(options, "operation");
    const directoryPath = readJsonStringField(options, "directory_path");

    if (operation === "move" && directoryPath) {
      return `Move the artifact into ${directoryPath}.`;
    }

    if (directoryPath) {
      return `Publish into ${directoryPath}.`;
    }

    return "Uses the destination default filesystem path.";
  }

  if (publishTargetKind === "itch") {
    const channel = readJsonStringField(options, "channel");
    const userversionTemplate = readJsonStringField(
      options,
      "userversion_template",
    );

    if (channel && userversionTemplate) {
      return `Channel ${channel}. Version template ${userversionTemplate}.`;
    }

    if (channel) {
      return `Channel ${channel}. Uses the git tag as the Itch userversion.`;
    }

    return "Uses the persisted Itch binding options.";
  }

  return "Uses the persisted binding options.";
}

function parseJsonObject(value: string): Record<string, unknown> | null {
  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }

  try {
    const parsed: unknown = JSON.parse(trimmed);
    if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
      return null;
    }

    return parsed as Record<string, unknown>;
  } catch {
    return null;
  }
}

function readJsonStringField(
  value: Record<string, unknown> | null,
  key: string,
) {
  const candidate = value?.[key];

  if (typeof candidate !== "string") {
    return null;
  }

  const trimmed = candidate.trim();
  return trimmed ? trimmed : null;
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
    repositoryVisibility: resolveRepositoryVisibilitySelection(repository),
    defaultBranch: repository.default_branch ?? "",
    artifactsRootOverride: repository.artifacts_root_override ?? "",
    workspaceRootOverride: repository.workspace_root_override ?? "",
    pollingIntervalSeconds: String(repository.polling_interval_seconds),
    enabled: repository.enabled ? "enabled" : "disabled",
    buildTargets,
  };
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
      target.unityExecutablePath.trim() === candidate.unityExecutablePath.trim()
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
