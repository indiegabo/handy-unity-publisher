import {
  Suspense,
  lazy,
  startTransition,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { useLocalization, type Translate } from "./LocalizationProvider";
import { Button, IconButton } from "./components/Button";
import { SelectField, TextField, type SelectOption } from "./components/Field";
import FullScreenModal from "./components/FullScreenModal";
import { type IconName } from "./components/Icon";
import { type AuthProviderConnectionResult } from "./components/authProviderPresentation";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { useOverlay } from "./components/OverlayManager";
import { ProcessFeedItem } from "./components/ProcessFeedItem";
import SelectListFullScreen, {
  type SelectListItem,
} from "./components/SelectListFullScreen";
import {
  WorkerStatusQuickView,
  type WorkerStatusQuickViewResult,
} from "./components/WorkerStatusQuickView";
import {
  WorkerStatusIndicator,
  type WorkerStatusTone,
} from "./components/WorkerStatusIndicator";
import { type ProcessFeedRecord } from "./components/processFeedPresentation";
import { type CreateProjectWizardSnapshot } from "./components/CreateProjectWizard";
import {
  type ProjectWorkerEntry,
  type RuntimeControlAction,
} from "./components/ProjectWorkersFocusScreen";
import {
  dispatchOnDemandReleaseProcess,
  loadRepositoryInspection,
  type OnDemandReleaseVersionSource,
  reconnectRepositoryAuth,
  type RepositoryInspectionEntry,
} from "./services/projects";
import {
  buildProjectSourceDisplay,
  isLocalWorkspaceSource,
} from "./projectSourcePresentation";
import { loginWithGithubAuth } from "./services/auth";
import {
  subscribeToProcessFeedEvents,
  type ProcessFeedRuntimeEvent,
} from "./services/processFeed";
import {
  subscribeToRuntimeEvents,
  type RuntimeEventRecord,
} from "./services/runtimeEvents";
import { rerunReleaseProcess } from "./services/processDetail";
import {
  loadRuntimeAutomationStatus,
  loadRuntimeHealth,
  requestRepositoryInstantCheck,
  restartRuntime,
  setRuntimeAutomationMode,
  startRuntime,
  stopRuntime,
  type RuntimeAutomationMode,
  type RuntimeHealthStatus,
} from "./services/runtime";

type ProcessFeedPage = {
  generated_at: string;
  page: number;
  page_size: number;
  total_items: number;
  total_pages: number;
  has_previous_page: boolean;
  has_next_page: boolean;
  items: ProcessFeedRecord[];
};

type ProcessFeedInput = {
  page: number;
  page_size: number;
};

type WorkerStatusSummary = {
  tone: WorkerStatusTone;
  label: string;
  animated: boolean;
};

type WorkerStatusSnapshot = {
  automationMode: RuntimeAutomationMode | null;
  repositories: RepositoryInspectionEntry[];
  inspectionAvailable: boolean;
  inspectionError: string | null;
  inspectionStale: boolean;
  runtimeStatus: RuntimeHealthStatus | null;
};

type ShellNavigationAction = {
  icon: IconName;
  label: string;
  onClick: () => void;
  variant: "primary" | "secondary" | "ghost";
};

type MainQuickReleaseDraft = {
  releaseVersion: string;
  versionSource: OnDemandReleaseVersionSource;
};

type MainQuickReleaseValidationErrors = {
  releaseVersion?: string;
};

type MainQuickReleaseOverlayResult =
  | {
      kind: "queued";
      gitTag: string;
    }
  | {
      kind: "open-project-list";
    };

type AppScreen =
  | { kind: "main" }
  | { kind: "create-project" }
  | {
      kind: "auth-providers";
      returnTo: "main" | "create-project" | "settings";
    }
  | { kind: "settings" }
  | { kind: "project-list"; highlightedRepositoryId: number | null }
  | { kind: "project-workers" }
  | {
      kind: "project-detail";
      repositoryId: number;
      returnTo: "main" | "project-list";
    }
  | { kind: "process-detail"; process: ProcessFeedRecord };

const PROCESS_FEED_PAGE_SIZE = 5;
const PRODUCT_NAME_FALLBACK = "Handy Games Publisher";
const EMPTY_PROCESS_FEED_PAGE: ProcessFeedPage = {
  generated_at: "",
  page: 1,
  page_size: PROCESS_FEED_PAGE_SIZE,
  total_items: 0,
  total_pages: 0,
  has_previous_page: false,
  has_next_page: false,
  items: [],
};
const EMPTY_WORKER_STATUS_SNAPSHOT: WorkerStatusSnapshot = {
  automationMode: null,
  repositories: [],
  inspectionAvailable: false,
  inspectionError: null,
  inspectionStale: false,
  runtimeStatus: null,
};
const WORKER_STATUS_REPOSITORY_EVENT_TOPICS = new Set<string>([
  "automation.release_queued",
  "build.run_started",
  "build.run_finished",
  "build.stage_updated",
  "publish.run_started",
  "publish.run_finished",
  "automation.poll_auth_failed",
]);
const MAIN_QUICK_RELEASE_VERSION_SOURCE_OPTIONS: SelectOption[] = [
  {
    label: "Manual version label",
    value: "manual",
  },
  {
    label: "Detect from project settings",
    value: "project_settings",
  },
];
const PROCESS_FEED_RESET_PAGE_EVENT_TOPICS = new Set<string>([
  "automation.release_queued",
  "build.run_started",
  "build.run_finished",
  "publish.run_started",
  "publish.run_finished",
]);

const AuthProvidersFocusScreen = lazy(() =>
  import("./components/AuthProvidersFocusScreen").then((module) => ({
    default: module.AuthProvidersFocusScreen,
  })),
);

const CreateProjectWizard = lazy(() =>
  import("./components/CreateProjectWizard").then((module) => ({
    default: module.CreateProjectWizard,
  })),
);

const ProcessDetailFocusScreen = lazy(() =>
  import("./components/ProcessDetailFocusScreen").then((module) => ({
    default: module.ProcessDetailFocusScreen,
  })),
);

const ProjectsFocusScreen = lazy(() =>
  import("./components/ProjectsFocusScreen").then((module) => ({
    default: module.ProjectsFocusScreen,
  })),
);

const ProjectWorkersFocusScreen = lazy(() =>
  import("./components/ProjectWorkersFocusScreen").then((module) => ({
    default: module.ProjectWorkersFocusScreen,
  })),
);

const RepositoryProjectDetail = lazy(() =>
  import("./components/RepositoryProjectDetail").then((module) => ({
    default: module.RepositoryProjectDetail,
  })),
);

const SettingsFocusScreen = lazy(() =>
  import("./components/SettingsFocusScreen").then((module) => ({
    default: module.SettingsFocusScreen,
  })),
);

// Shell routing stays local to App so the audited entry points remain explicit.
// Navigation enters through the home action bars, process-feed detail actions,
// focus-screen back handling, and overlay result hand-offs. Overlay entry
// points are the worker quick view, runtime lifecycle confirmations, bulk
// instant-check selection and confirmation, and the create-project discard
// guard.
function App() {
  const { t } = useLocalization();
  const { dismissTopOverlay, hasOpenOverlay, openOverlay } = useOverlay();
  const [page, setPage] = useState(1);
  const [activeScreen, setActiveScreen] = useState<AppScreen>({ kind: "main" });
  const [activeProjectTitle, setActiveProjectTitle] = useState<string | null>(
    null,
  );
  const [isMainWindowPinned, setIsMainWindowPinned] = useState(false);
  const [isScreenBlank, setIsScreenBlank] = useState(false);
  const [transitionError, setTransitionError] = useState<string | null>(null);
  const [processPage, setProcessPage] = useState<ProcessFeedPage>(
    EMPTY_PROCESS_FEED_PAGE,
  );
  const [isLoadingFeed, setIsLoadingFeed] = useState(true);
  const [, setIsRefreshingFeed] = useState(false);
  const [feedError, setFeedError] = useState<string | null>(null);
  const [workerSnapshot, setWorkerSnapshot] = useState<WorkerStatusSnapshot>(
    EMPTY_WORKER_STATUS_SNAPSHOT,
  );
  const [workerActionError, setWorkerActionError] = useState<string | null>(
    null,
  );
  const [mainQuickReleaseMessage, setMainQuickReleaseMessage] = useState<
    string | null
  >(null);
  const [workerActionMessage, setWorkerActionMessage] = useState<string | null>(
    null,
  );
  const [createProjectWizardSnapshot, setCreateProjectWizardSnapshot] =
    useState<CreateProjectWizardSnapshot | null>(null);
  const [createProjectAuthProviderResult, setCreateProjectAuthProviderResult] =
    useState<AuthProviderConnectionResult | null>(null);
  const [isCreateProjectWizardDirty, setIsCreateProjectWizardDirty] =
    useState(false);
  const [pendingBulkInstantCheck, setPendingBulkInstantCheck] = useState(false);
  const [pendingRuntimeAction, setPendingRuntimeAction] =
    useState<RuntimeControlAction | null>(null);
  const [pendingAutomationMode, setPendingAutomationMode] =
    useState<RuntimeAutomationMode | null>(null);
  const [pendingInstantCheckRepositoryId, setPendingInstantCheckRepositoryId] =
    useState<number | null>(null);
  const latestRequestIdRef = useRef(0);
  const latestWorkerStatusRequestIdRef = useRef(0);
  const isNavigatingRef = useRef(false);
  const githubReauthPromptedRepositoryIdsRef = useRef<Set<number>>(new Set());
  const githubReauthPromptInFlightRef = useRef(false);
  const productName = t("app.product_name", PRODUCT_NAME_FALLBACK);
  const projectWorkers = collectProjectWorkers(workerSnapshot.repositories);
  const workerStatus = resolveWorkerStatusSummary(t, workerSnapshot);
  const workerStatusDescription = buildWorkerStatusDescription(
    t,
    workerSnapshot,
    projectWorkers,
  );
  const workerStatusTooltip = buildWorkerStatusTooltip(
    t,
    workerSnapshot.automationMode,
    projectWorkers,
  );
  const activeProcessDetail =
    activeScreen.kind === "process-detail"
      ? (processPage.items.find(
          (process) =>
            process.release_run_id === activeScreen.process.release_run_id,
        ) ?? activeScreen.process)
      : null;
  const activeProcessDetailUsesLiveSnapshot =
    activeScreen.kind === "process-detail" &&
    processPage.items.some(
      (process) =>
        process.release_run_id === activeScreen.process.release_run_id,
    );
  const isInCreateProjectFlow =
    activeScreen.kind === "create-project" ||
    (activeScreen.kind === "auth-providers" &&
      activeScreen.returnTo === "create-project");

  useEffect(() => {
    if (isInCreateProjectFlow) {
      return;
    }

    if (
      !createProjectWizardSnapshot &&
      !createProjectAuthProviderResult &&
      !isCreateProjectWizardDirty
    ) {
      return;
    }

    startTransition(() => {
      setCreateProjectAuthProviderResult(null);
      setCreateProjectWizardSnapshot(null);
      setIsCreateProjectWizardDirty(false);
    });
  }, [
    createProjectAuthProviderResult,
    createProjectWizardSnapshot,
    isCreateProjectWizardDirty,
    isInCreateProjectFlow,
  ]);

  const loadProcessFeed = useEffectEvent(
    async (pageToLoad: number, reason: "page" | "event") => {
      const requestId = latestRequestIdRef.current + 1;
      latestRequestIdRef.current = requestId;

      if (reason === "event") {
        setIsRefreshingFeed(true);
      } else {
        setIsLoadingFeed(true);
      }

      try {
        const response = await invoke<ProcessFeedPage>("process_feed", {
          input: {
            page: pageToLoad,
            page_size: PROCESS_FEED_PAGE_SIZE,
          } satisfies ProcessFeedInput,
        });

        if (requestId !== latestRequestIdRef.current) {
          return;
        }

        startTransition(() => {
          setProcessPage(response);
          setFeedError(null);
          setIsLoadingFeed(false);
          setIsRefreshingFeed(false);
          setPage(response.page);
        });
      } catch (error) {
        if (requestId !== latestRequestIdRef.current) {
          return;
        }

        startTransition(() => {
          setFeedError(buildInvokeErrorMessage(error));
          setIsLoadingFeed(false);
          setIsRefreshingFeed(false);
        });
      }
    },
  );

  const loadMainWindowPinState = useEffectEvent(async () => {
    try {
      const pinned = await invoke<boolean>("main_window_pin_state");
      startTransition(() => {
        setIsMainWindowPinned(pinned);
      });
    } catch (error) {
      console.error("failed to load main window pin state", error);
    }
  });

  useEffect(() => {
    void loadMainWindowPinState();
    void loadProcessFeed(page, "page");
  }, [page]);

  async function maybeRecoverGithubRepositoryAuth(
    repositories: RepositoryInspectionEntry[],
  ) {
    const reauthRepositoryIds = new Set(
      repositories
        .filter(
          (repository) =>
            repository.source_provider_id === "github" &&
            repository.auth_binding_status === "reauth_required",
        )
        .map((repository) => repository.repository_id),
    );

    for (const repositoryId of githubReauthPromptedRepositoryIdsRef.current) {
      if (!reauthRepositoryIds.has(repositoryId)) {
        githubReauthPromptedRepositoryIdsRef.current.delete(repositoryId);
      }
    }

    if (githubReauthPromptInFlightRef.current) {
      return;
    }

    const repository = repositories.find(
      (candidate) =>
        candidate.source_provider_id === "github" &&
        candidate.auth_binding_status === "reauth_required" &&
        !githubReauthPromptedRepositoryIdsRef.current.has(
          candidate.repository_id,
        ),
    );
    if (!repository) {
      return;
    }

    githubReauthPromptedRepositoryIdsRef.current.add(repository.repository_id);
    githubReauthPromptInFlightRef.current = true;

    try {
      const provider = await loginWithGithubAuth({ force: true });
      const credentialId =
        provider.credential_id ?? repository.credentials?.credential_id ?? null;
      if (!credentialId) {
        throw new Error(
          "GitHub relogin completed without a reusable credential id.",
        );
      }

      await reconnectRepositoryAuth(repository.repository_id, credentialId);
      await loadWorkerRepositories();
    } catch (error) {
      console.error(
        "failed to recover GitHub authentication for repository",
        repository.repository_id,
        error,
      );
    } finally {
      githubReauthPromptInFlightRef.current = false;
    }
  }

  const loadWorkerRepositories = useEffectEvent(async () => {
    const requestId = latestWorkerStatusRequestIdRef.current + 1;
    latestWorkerStatusRequestIdRef.current = requestId;

    const inspectionResult = await loadRepositoryInspection()
      .then((value) => ({ status: "fulfilled" as const, value }))
      .catch((reason) => ({ status: "rejected" as const, reason }));

    if (requestId !== latestWorkerStatusRequestIdRef.current) {
      return;
    }

    startTransition(() => {
      setWorkerSnapshot((current) => ({
        ...current,
        repositories:
          inspectionResult.status === "fulfilled"
            ? inspectionResult.value.repositories
            : current.repositories,
        inspectionAvailable:
          inspectionResult.status === "fulfilled"
            ? true
            : current.inspectionAvailable,
        inspectionError:
          inspectionResult.status === "fulfilled"
            ? null
            : buildWorkerInspectionErrorMessage(inspectionResult.reason),
        inspectionStale:
          inspectionResult.status === "fulfilled"
            ? false
            : current.inspectionAvailable,
      }));
    });

    if (inspectionResult.status === "fulfilled") {
      await maybeRecoverGithubRepositoryAuth(
        inspectionResult.value.repositories,
      );
    }
  });

  const loadRuntimeStatus = useEffectEvent(async () => {
    const [healthResult, automationResult] = await Promise.all([
      loadRuntimeHealth()
        .then((value) => ({ status: "fulfilled" as const, value }))
        .catch((reason) => ({ status: "rejected" as const, reason })),
      loadRuntimeAutomationStatus()
        .then((value) => ({ status: "fulfilled" as const, value }))
        .catch((reason) => ({ status: "rejected" as const, reason })),
    ]);

    startTransition(() => {
      setWorkerSnapshot((current) => ({
        ...current,
        automationMode:
          automationResult.status === "fulfilled"
            ? automationResult.value.mode
            : current.automationMode,
        runtimeStatus:
          healthResult.status === "fulfilled"
            ? healthResult.value.status
            : null,
      }));
    });
  });

  const loadWorkerStatus = useEffectEvent(async () => {
    await Promise.all([loadWorkerRepositories(), loadRuntimeStatus()]);
  });

  useEffect(() => {
    void loadWorkerStatus();
  }, []);

  const handleRuntimeStatusEvent = useEffectEvent(
    (event: RuntimeEventRecord) => {
      if (event.topic === "automation.mode_changed") {
        const mode = event.payload.mode;

        if (mode !== "active" && mode !== "idle") {
          return;
        }

        startTransition(() => {
          setWorkerSnapshot((current) => ({
            ...current,
            automationMode: mode,
          }));
        });
        return;
      }

      if (event.topic !== "runtime.status_changed") {
        return;
      }

      const status = event.payload.status;
      if (
        status !== "bootstrapping" &&
        status !== "healthy" &&
        status !== "shutting_down" &&
        status !== "stopped" &&
        status !== "unhealthy"
      ) {
        return;
      }

      startTransition(() => {
        setWorkerSnapshot((current) => ({
          ...current,
          runtimeStatus: status,
        }));
      });
    },
  );

  const handleProcessFeedEvent = useEffectEvent(
    (event: ProcessFeedRuntimeEvent) => {
      if (PROCESS_FEED_RESET_PAGE_EVENT_TOPICS.has(event.topic) && page !== 1) {
        startTransition(() => {
          setPage(1);
        });
        return;
      }

      void loadProcessFeed(page, "event");
    },
  );

  useEffect(() => {
    let disposed = false;
    let unsubscribe: (() => void) | undefined;

    void subscribeToProcessFeedEvents((event) => {
      if (disposed) {
        return;
      }

      handleProcessFeedEvent(event);
    })
      .then((dispose) => {
        if (disposed) {
          dispose();
          return;
        }

        unsubscribe = dispose;
      })
      .catch((error: unknown) => {
        if (disposed) {
          return;
        }

        setFeedError(buildEventSubscriptionErrorMessage(error));
      });

    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    let unsubscribe: (() => void) | undefined;

    void subscribeToRuntimeEvents((event) => {
      if (disposed) {
        return;
      }

      handleRuntimeStatusEvent(event);

      if (!WORKER_STATUS_REPOSITORY_EVENT_TOPICS.has(event.topic)) {
        return;
      }

      void loadWorkerRepositories();
    })
      .then((dispose) => {
        if (disposed) {
          dispose();
          return;
        }

        unsubscribe = dispose;
      })
      .catch((error: unknown) => {
        if (disposed) {
          return;
        }

        setFeedError(buildEventSubscriptionErrorMessage(error));
      });

    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, []);

  const transitionToScreen = useEffectEvent(async (nextScreen: AppScreen) => {
    if (isNavigatingRef.current) {
      return;
    }

    isNavigatingRef.current = true;
    setTransitionError(null);
    setIsScreenBlank(true);

    try {
      await waitForShellTransitionPhase();
      await invoke("transition_window_focus", {
        target: nextScreen.kind === "main" ? "main" : "focus",
      });

      startTransition(() => {
        setActiveScreen(nextScreen);
        setIsScreenBlank(false);
      });
    } catch (error) {
      startTransition(() => {
        setTransitionError(buildWindowTransitionErrorMessage(error));
        setIsScreenBlank(false);
      });
    } finally {
      isNavigatingRef.current = false;
    }
  });

  const handleOpenProcessDetail = useEffectEvent(
    (process: ProcessFeedRecord) => {
      void transitionToScreen({
        kind: "process-detail",
        process,
      });
    },
  );

  const handleRerunProcess = useEffectEvent(
    async (process: ProcessFeedRecord) => {
      await rerunReleaseProcess(process.release_run_id);
      await loadProcessFeed(1, "event");
      await loadWorkerStatus();
      await transitionToScreen({ kind: "main" });
    },
  );

  const handleToggleMainWindowPinned = useEffectEvent(async () => {
    try {
      const pinned = await invoke<boolean>("set_main_window_pinned", {
        pinned: !isMainWindowPinned,
      });

      startTransition(() => {
        setIsMainWindowPinned(pinned);
      });
    } catch (error) {
      console.error("failed to toggle main window pin state", error);
    }
  });

  const closeMainWindow = useEffectEvent(async () => {
    try {
      await invoke("close_main_window");
    } catch (error) {
      console.error("failed to close main window", error);
    }
  });

  const handleOpenCreateProject = useEffectEvent(() => {
    void transitionToScreen({ kind: "create-project" });
  });

  const handleOpenAuthProviders = useEffectEvent(() => {
    void transitionToScreen({
      kind: "auth-providers",
      returnTo: "main",
    });
  });

  const handleOpenAuthProvidersFromSettings = useEffectEvent(() => {
    void transitionToScreen({
      kind: "auth-providers",
      returnTo: "settings",
    });
  });

  const handleOpenAuthProvidersFromWizard = useEffectEvent(() => {
    startTransition(() => {
      setCreateProjectAuthProviderResult(null);
    });

    void transitionToScreen({
      kind: "auth-providers",
      returnTo: "create-project",
    });
  });

  const handleAuthProviderResult = useEffectEvent(
    (result: AuthProviderConnectionResult) => {
      if (
        activeScreen.kind !== "auth-providers" ||
        activeScreen.returnTo !== "create-project"
      ) {
        return;
      }

      startTransition(() => {
        setCreateProjectAuthProviderResult(result);
      });
    },
  );

  const handleOpenSettings = useEffectEvent(() => {
    void transitionToScreen({ kind: "settings" });
  });

  const handleOpenProjects = useEffectEvent(() => {
    void transitionToScreen({
      kind: "project-list",
      highlightedRepositoryId: null,
    });
  });

  const handleOpenMainQuickRelease = useEffectEvent(async () => {
    const repositories = workerSnapshot.repositories;

    const selectedRepositoryId = await openOverlay<string>(
      SelectListFullScreen,
      {
        description:
          "Choose one registered project to open the quick release start flow.",
        emptyStateCopy:
          "Register a project first, then return here to queue a release from the main screen.",
        emptyStateTitle: "No registered projects available.",
        items: repositories.map((repository) => ({
          id: String(repository.repository_id),
          label: repository.repository_name,
          subtitle: buildProjectSourceDisplay(repository),
        })),
        title: "Start release",
      },
    );

    if (!selectedRepositoryId) {
      return;
    }

    const selectedRepository = repositories.find(
      (repository) => String(repository.repository_id) === selectedRepositoryId,
    );

    if (!selectedRepository) {
      return;
    }

    const result = await openOverlay<MainQuickReleaseOverlayResult>(
      MainQuickReleaseOverlay,
      {
        repository: selectedRepository,
      },
    );

    if (!result) {
      return;
    }

    if (result.kind === "open-project-list") {
      handleOpenProjects();
      return;
    }

    startTransition(() => {
      setMainQuickReleaseMessage(
        `Queued local release ${result.gitTag} for ${selectedRepository.repository_name}.`,
      );
    });

    await loadWorkerStatus();
    startTransition(() => {
      setPage(1);
    });
    await loadProcessFeed(1, "event");
  });

  const handleProjectRemoved = useEffectEvent(() => {
    startTransition(() => {
      setActiveProjectTitle(null);
    });

    void transitionToScreen({
      kind: "project-list",
      highlightedRepositoryId: null,
    });
  });

  const handleOpenProjectDetail = useEffectEvent(
    (repositoryId: number, repositoryName?: string) => {
      startTransition(() => {
        setActiveProjectTitle(repositoryName?.trim() || null);
      });

      void transitionToScreen({
        kind: "project-detail",
        repositoryId,
        returnTo: "project-list",
      });
    },
  );

  const handleProjectNameResolved = useEffectEvent((repositoryName: string) => {
    const normalizedName = repositoryName.trim();

    if (!normalizedName) {
      return;
    }

    startTransition(() => {
      setActiveProjectTitle(normalizedName);
    });
  });

  const handleOpenProjectWorkers = useEffectEvent(() => {
    setWorkerActionError(null);
    setWorkerActionMessage(null);
    void loadWorkerStatus();
    void transitionToScreen({ kind: "project-workers" });
  });

  const handleRetryWorkerInventory = useEffectEvent(() => {
    setWorkerActionError(null);
    setWorkerActionMessage(null);
    void loadWorkerStatus();
  });

  const handleOpenWorkerQuickView = useEffectEvent(async () => {
    void loadWorkerStatus();

    const result = await openOverlay<WorkerStatusQuickViewResult>(
      WorkerStatusQuickView,
      {
        automationMode: workerSnapshot.automationMode,
        inspectionAvailable: workerSnapshot.inspectionAvailable,
        projectWorkers,
        runtimeStatus: workerSnapshot.runtimeStatus,
      },
    );

    if (result === "open-project-workers") {
      handleOpenProjectWorkers();
    }
  });

  const confirmRuntimeLifecycleAction = useEffectEvent(
    async (action: Extract<RuntimeControlAction, "stop" | "restart">) => {
      const shouldContinue = await openOverlay<boolean>(ConfirmDialog, {
        cancelLabel:
          action === "stop" ? "Keep runtime online" : "Keep current state",
        confirmLabel: action === "stop" ? "Stop runtime" : "Restart runtime",
        confirmVariant: "secondary",
        description:
          action === "stop"
            ? "Stopping the runtime pauses project workers until the local host is started again."
            : "Restarting the runtime interrupts active worker supervision while the local host comes back up.",
        message:
          action === "stop"
            ? "Project workers will remain unavailable until the runtime is started again."
            : "Project workers will briefly disconnect while the runtime restarts.",
        title: action === "stop" ? "Stop runtime?" : "Restart runtime?",
      });

      if (!shouldContinue) {
        return;
      }

      await runRuntimeLifecycleAction(action);
    },
  );

  const runRuntimeLifecycleAction = useEffectEvent(
    async (action: RuntimeControlAction) => {
      setWorkerActionError(null);
      setWorkerActionMessage(null);
      setPendingRuntimeAction(action);

      try {
        switch (action) {
          case "start":
            await startRuntime();
            break;
          case "stop":
            await stopRuntime();
            break;
          case "restart":
            await restartRuntime();
            break;
        }

        await loadWorkerStatus();
        startTransition(() => {
          setWorkerActionMessage(buildRuntimeActionMessage(action));
        });
      } catch (error) {
        startTransition(() => {
          setWorkerActionError(buildRuntimeActionErrorMessage(error, action));
        });
      } finally {
        startTransition(() => {
          setPendingRuntimeAction(null);
        });
      }
    },
  );

  const handleStartRuntime = useEffectEvent(() => {
    void runRuntimeLifecycleAction("start");
  });

  const handleStopRuntime = useEffectEvent(() => {
    void confirmRuntimeLifecycleAction("stop");
  });

  const handleRestartRuntime = useEffectEvent(() => {
    void confirmRuntimeLifecycleAction("restart");
  });

  const handleToggleRuntimeAutomationMode = useEffectEvent(async () => {
    const nextMode =
      workerSnapshot.automationMode === "idle" ? "active" : "idle";

    setWorkerActionError(null);
    setWorkerActionMessage(null);
    setPendingAutomationMode(nextMode);

    try {
      const snapshot = await setRuntimeAutomationMode(nextMode);

      startTransition(() => {
        setWorkerSnapshot((current) => ({
          ...current,
          automationMode: snapshot.mode,
        }));
        setWorkerActionMessage(buildRuntimeAutomationMessage(snapshot.mode));
      });
    } catch (error) {
      startTransition(() => {
        setWorkerActionError(
          buildRuntimeAutomationErrorMessage(error, nextMode),
        );
      });
    } finally {
      startTransition(() => {
        setPendingAutomationMode(null);
      });
    }
  });

  const handleBulkRepositoryInstantCheck = useEffectEvent(async () => {
    if (
      pendingBulkInstantCheck ||
      pendingInstantCheckRepositoryId !== null ||
      projectWorkers.length === 0
    ) {
      return;
    }

    const selectedRepositoryIds = await openOverlay<string[]>(
      SelectListFullScreen,
      {
        description:
          "Select one or more project workers and queue an immediate repository check for each of them.",
        emptyStateCopy:
          "Refresh the worker inventory if the expected repositories are still missing.",
        emptyStateTitle: "No project workers matched the current filter.",
        items: buildProjectWorkerSelectionItems(projectWorkers),
        selectionLabel: "Selected",
        selectionMode: "multiple",
        submitLabel: "Review queued checks",
        title: "Queue instant checks",
      },
    );

    if (!selectedRepositoryIds || selectedRepositoryIds.length === 0) {
      return;
    }

    const selectedWorkers = projectWorkers.filter((projectWorker) =>
      selectedRepositoryIds.includes(String(projectWorker.repositoryId)),
    );

    if (selectedWorkers.length === 0) {
      return;
    }

    const shouldQueue = await openOverlay<boolean>(ConfirmDialog, {
      cancelLabel: "Back to selection",
      confirmLabel:
        selectedWorkers.length === 1 ? "Queue check" : "Queue checks",
      confirmVariant: "primary",
      description:
        "Queue an immediate repository check for each selected worker in sequence.",
      message: buildBulkInstantCheckConfirmationMessage(selectedWorkers),
      title: "Queue instant checks?",
    });

    if (!shouldQueue) {
      return;
    }

    let activeWorker: ProjectWorkerEntry | null = null;
    const queuedWorkers: ProjectWorkerEntry[] = [];

    setWorkerActionError(null);
    setWorkerActionMessage(null);
    setPendingBulkInstantCheck(true);

    try {
      for (const projectWorker of selectedWorkers) {
        activeWorker = projectWorker;
        setPendingInstantCheckRepositoryId(projectWorker.repositoryId);
        await requestRepositoryInstantCheck(projectWorker.repositoryId);
        queuedWorkers.push(projectWorker);
      }

      await loadWorkerStatus();
      startTransition(() => {
        setWorkerActionMessage(buildBulkInstantCheckMessage(queuedWorkers));
      });
    } catch (error) {
      startTransition(() => {
        setWorkerActionError(
          buildBulkRepositoryInstantCheckErrorMessage(
            error,
            activeWorker?.repositoryName ?? null,
            queuedWorkers.length,
          ),
        );
      });
    } finally {
      startTransition(() => {
        setPendingBulkInstantCheck(false);
        setPendingInstantCheckRepositoryId(null);
      });
    }
  });

  const handleRepositoryInstantCheck = useEffectEvent(
    async (repositoryId: number, repositoryName: string) => {
      setWorkerActionError(null);
      setWorkerActionMessage(null);
      setPendingInstantCheckRepositoryId(repositoryId);

      try {
        await requestRepositoryInstantCheck(repositoryId);
        await loadWorkerStatus();
        startTransition(() => {
          setWorkerActionMessage(`Instant check queued for ${repositoryName}.`);
        });
      } catch (error) {
        startTransition(() => {
          setWorkerActionError(
            buildRepositoryInstantCheckErrorMessage(error, repositoryName),
          );
        });
      } finally {
        startTransition(() => {
          setPendingInstantCheckRepositoryId(null);
        });
      }
    },
  );

  const handleProjectCreated = useEffectEvent((repositoryId: number) => {
    startTransition(() => {
      setCreateProjectWizardSnapshot(null);
      setIsCreateProjectWizardDirty(false);
      setActiveScreen({
        kind: "project-list",
        highlightedRepositoryId: repositoryId,
      });
    });
  });

  const handleReturnFromFocus = useEffectEvent(() => {
    if (
      activeScreen.kind === "project-detail" &&
      activeScreen.returnTo === "project-list"
    ) {
      void transitionToScreen({
        kind: "project-list",
        highlightedRepositoryId: null,
      });
      return;
    }

    if (
      activeScreen.kind === "auth-providers" &&
      activeScreen.returnTo === "create-project"
    ) {
      void transitionToScreen({ kind: "create-project" });
      return;
    }

    if (
      activeScreen.kind === "auth-providers" &&
      activeScreen.returnTo === "settings"
    ) {
      void transitionToScreen({ kind: "settings" });
      return;
    }

    void transitionToScreen({ kind: "main" });
  });

  const handleRequestCreateProjectClose = useEffectEvent(async () => {
    if (activeScreen.kind !== "create-project") {
      handleReturnFromFocus();
      return;
    }

    if (isCreateProjectWizardDirty) {
      const shouldDiscard = await openOverlay<boolean>(ConfirmDialog, {
        cancelLabel: "Continue editing",
        confirmLabel: "Discard draft",
        confirmVariant: "secondary",
        description:
          "This leaves project creation and clears the current unsaved wizard draft.",
        message:
          "HGP will discard the repository, target, publish, and path changes that have not been saved yet.",
        title: "Discard project draft?",
      });

      if (!shouldDiscard) {
        return;
      }
    }

    startTransition(() => {
      setCreateProjectWizardSnapshot(null);
      setIsCreateProjectWizardDirty(false);
    });

    handleReturnFromFocus();
  });

  const handleFocusBackAction = useEffectEvent(() => {
    if (hasOpenOverlay && dismissTopOverlay()) {
      return;
    }

    if (activeScreen.kind === "create-project") {
      void handleRequestCreateProjectClose();
      return;
    }

    handleReturnFromFocus();
  });

  const handleRequestShellClose = useEffectEvent(async () => {
    await closeMainWindow();
  });

  const homePrimaryNavigationActions: ShellNavigationAction[] = [
    {
      icon: "layout",
      label: t("app.main.navigation.projects", "Projects"),
      onClick: handleOpenProjects,
      variant: "secondary" as const,
    },
    {
      icon: "plus",
      label: t("app.main.navigation.create_project", "Create project"),
      onClick: handleOpenCreateProject,
      variant: "primary" as const,
    },
  ];
  const homeSecondaryNavigationActions: ShellNavigationAction[] = [
    {
      icon: "key",
      label: t("app.main.navigation.auth", "Auth"),
      onClick: handleOpenAuthProviders,
      variant: "ghost" as const,
    },
    {
      icon: "settings",
      label: t("app.main.navigation.settings", "Settings"),
      onClick: handleOpenSettings,
      variant: "ghost" as const,
    },
  ];
  const focusBackLabel = resolveFocusBackLabel(t, activeScreen);
  const focusScreenShellClassName =
    resolveFocusScreenShellClassName(activeScreen);

  useEffect(() => {
    const handleWindowKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.key !== "Escape" || !hasOpenOverlay) {
        return;
      }

      if (!dismissTopOverlay()) {
        return;
      }

      event.preventDefault();
      event.stopPropagation();
    };

    window.addEventListener("keydown", handleWindowKeyDown);

    return () => {
      window.removeEventListener("keydown", handleWindowKeyDown);
    };
  }, [dismissTopOverlay, hasOpenOverlay]);

  return (
    <main className="app-shell">
      <header className="window-titlebar">
        <div
          className="window-titlebar__drag-region"
          data-tauri-drag-region
          onMouseDown={(event) => {
            if (event.button !== 0) {
              return;
            }

            void getCurrentWindow()
              .startDragging()
              .catch((error) => {
                console.error("failed to start window drag", error);
              });
          }}
        >
          <span className="window-titlebar__title">
            {resolveWindowTitle(
              t,
              productName,
              activeScreen,
              activeProjectTitle,
            )}
          </span>
        </div>
        <div className="window-titlebar__actions">
          <IconButton
            aria-pressed={isMainWindowPinned}
            className="window-titlebar__action window-titlebar__action--pin"
            icon={isMainWindowPinned ? "unpin" : "pin"}
            label={
              isMainWindowPinned
                ? t("app.window.unpin", "Unpin window")
                : t("app.window.pin", "Pin window")
            }
            onClick={handleToggleMainWindowPinned}
            size="sm"
            variant="ghost"
          />
          <IconButton
            className="window-titlebar__action window-titlebar__action--close"
            icon="close"
            label={t("app.window.close", "Close window")}
            onClick={handleRequestShellClose}
            size="sm"
            variant="ghost"
          />
        </div>
      </header>

      <div className="app-shell__content">
        {isScreenBlank ? null : activeScreen.kind === "main" ? (
          <div className="home-frame">
            <section
              className="action-bar"
              aria-label={t("app.main.primary_actions", "Primary actions")}
            >
              <div className="action-bar__leading">
                <div className="worker-status-shell">
                  <WorkerStatusIndicator
                    animated={workerStatus.animated}
                    aria-description={workerStatusDescription}
                    aria-haspopup="dialog"
                    label={workerStatus.label}
                    onClick={() => {
                      void handleOpenWorkerQuickView();
                    }}
                    tone={workerStatus.tone}
                    title={workerStatusTooltip}
                  />
                  <Button
                    className="worker-status-shell__automation-toggle"
                    disabled={
                      pendingAutomationMode !== null ||
                      workerSnapshot.automationMode === null
                    }
                    leadingIcon={
                      workerSnapshot.automationMode === "idle"
                        ? "play"
                        : undefined
                    }
                    onClick={() => {
                      void handleToggleRuntimeAutomationMode();
                    }}
                    size="sm"
                    variant={
                      workerSnapshot.automationMode === "idle"
                        ? "primary"
                        : "secondary"
                    }
                  >
                    {resolveAutomationToggleLabel(
                      t,
                      workerSnapshot.automationMode,
                      pendingAutomationMode,
                    )}
                  </Button>
                </div>
              </div>

              <div className="action-bar__actions">
                <Button
                  leadingIcon="queue"
                  onClick={() => {
                    void handleOpenMainQuickRelease();
                  }}
                  size="sm"
                  variant="secondary"
                >
                  {t("app.main.navigation.start_release", "Start release")}
                </Button>
                {homePrimaryNavigationActions.map((action) => (
                  <IconButton
                    icon={action.icon}
                    key={action.label}
                    label={action.label}
                    onClick={action.onClick}
                    size="sm"
                    variant={action.variant}
                  />
                ))}
              </div>
            </section>

            {mainQuickReleaseMessage ? (
              <p className="notice-banner">{mainQuickReleaseMessage}</p>
            ) : null}

            <section
              className="process-feed-shell"
              aria-label={t("app.main.process_list", "Process list")}
            >
              {transitionError ? (
                <p className="feed-banner feed-banner--error">
                  {transitionError}
                </p>
              ) : null}

              {feedError ? (
                <p className="feed-banner feed-banner--error">{feedError}</p>
              ) : null}

              {isLoadingFeed && processPage.items.length === 0 ? (
                <div className="feed-state">
                  <p className="feed-state__title">
                    {t(
                      "app.main.feed.loading.title",
                      "Loading process feed...",
                    )}
                  </p>
                  <p className="feed-state__copy">
                    {t(
                      "app.main.feed.loading.copy",
                      "The shell is querying the runtime for recent build and publishing activity.",
                    )}
                  </p>
                </div>
              ) : null}

              {!isLoadingFeed && processPage.items.length === 0 ? (
                <div className="feed-state">
                  <p className="feed-state__title">
                    {t(
                      "app.main.feed.empty.title",
                      "No processes recorded yet.",
                    )}
                  </p>
                  <p className="feed-state__copy">
                    {t(
                      "app.main.feed.empty.copy",
                      "New build or publishing runs will appear here as soon as the runtime creates them.",
                    )}
                  </p>
                </div>
              ) : null}

              {processPage.items.length > 0 ? (
                <div className="process-list" aria-live="polite">
                  {processPage.items.map((process) => (
                    <ProcessFeedItem
                      key={process.release_run_id}
                      onOpenDetail={handleOpenProcessDetail}
                      process={process}
                    />
                  ))}
                </div>
              ) : null}

              {processPage.total_pages > 1 ? (
                <footer className="pagination-bar">
                  <div className="pagination-bar__actions">
                    <Button
                      disabled={!processPage.has_previous_page || isLoadingFeed}
                      onClick={() =>
                        startTransition(() => {
                          setPage(processPage.page - 1);
                        })
                      }
                      size="sm"
                      variant="ghost"
                    >
                      {t("app.main.pagination.previous", "Previous")}
                    </Button>
                    <Button
                      disabled={!processPage.has_next_page || isLoadingFeed}
                      onClick={() =>
                        startTransition(() => {
                          setPage(processPage.page + 1);
                        })
                      }
                      size="sm"
                      variant="secondary"
                    >
                      {t("app.main.pagination.next", "Next")}
                    </Button>
                  </div>
                </footer>
              ) : null}
            </section>

            <section
              className="action-bar action-bar--home-bottom"
              aria-label={t("app.main.secondary_actions", "Secondary actions")}
            >
              <div className="action-bar__actions">
                {homeSecondaryNavigationActions.map((action) => (
                  <IconButton
                    icon={action.icon}
                    key={action.label}
                    label={action.label}
                    onClick={action.onClick}
                    size="sm"
                    variant={action.variant}
                  />
                ))}
              </div>
            </section>
          </div>
        ) : (
          <div className="focus-frame">
            <section
              className="action-bar action-bar--focus"
              aria-label={t("app.main.focus_actions", "Focus actions")}
            >
              <div className="action-bar__actions action-bar__actions--leading">
                <IconButton
                  icon="arrowLeft"
                  label={focusBackLabel}
                  onClick={handleFocusBackAction}
                  size="sm"
                  variant="ghost"
                />
              </div>
            </section>

            <section
              className={focusScreenShellClassName}
              aria-label={t("app.main.focus_screen", "Focus screen")}
            >
              <Suspense fallback={null}>
                {transitionError ? (
                  <p className="feed-banner feed-banner--error">
                    {transitionError}
                  </p>
                ) : null}

                {activeScreen.kind === "create-project" ? (
                  <CreateProjectWizard
                    authProviderResult={createProjectAuthProviderResult}
                    initialSnapshot={createProjectWizardSnapshot}
                    onCreated={handleProjectCreated}
                    onDirtyChange={setIsCreateProjectWizardDirty}
                    onManageAuth={handleOpenAuthProvidersFromWizard}
                    onRequestClose={handleRequestCreateProjectClose}
                    onSnapshotChange={setCreateProjectWizardSnapshot}
                  />
                ) : null}

                {activeScreen.kind === "auth-providers" ? (
                  <AuthProvidersFocusScreen
                    onResult={handleAuthProviderResult}
                  />
                ) : null}

                {activeScreen.kind === "settings" ? (
                  <SettingsFocusScreen
                    automationMode={workerSnapshot.automationMode}
                    onManageAuthProviders={handleOpenAuthProvidersFromSettings}
                    onOpenProjectWorkers={handleOpenProjectWorkers}
                    onOpenProjects={handleOpenProjects}
                  />
                ) : null}

                {activeScreen.kind === "project-list" ? (
                  <ProjectsFocusScreen
                    highlightedRepositoryId={
                      activeScreen.highlightedRepositoryId
                    }
                    onOpenProject={handleOpenProjectDetail}
                  />
                ) : null}

                {activeScreen.kind === "project-workers" ? (
                  <ProjectWorkersFocusScreen
                    actionError={workerActionError}
                    actionMessage={workerActionMessage}
                    automationMode={workerSnapshot.automationMode}
                    inspectionAvailable={workerSnapshot.inspectionAvailable}
                    inspectionError={workerSnapshot.inspectionError}
                    inspectionStale={workerSnapshot.inspectionStale}
                    onBulkInstantCheck={handleBulkRepositoryInstantCheck}
                    onInstantCheck={handleRepositoryInstantCheck}
                    onRestartRuntime={handleRestartRuntime}
                    onRetryInventory={handleRetryWorkerInventory}
                    onStartRuntime={handleStartRuntime}
                    onStopRuntime={handleStopRuntime}
                    pendingBulkInstantCheck={pendingBulkInstantCheck}
                    pendingInstantCheckRepositoryId={
                      pendingInstantCheckRepositoryId
                    }
                    pendingRuntimeAction={pendingRuntimeAction}
                    projectWorkers={projectWorkers}
                    runtimeStatus={workerSnapshot.runtimeStatus}
                  />
                ) : null}

                {activeScreen.kind === "project-detail" ? (
                  <RepositoryProjectDetail
                    onProjectNameResolved={handleProjectNameResolved}
                    onProjectRemoved={handleProjectRemoved}
                    repositoryId={activeScreen.repositoryId}
                  />
                ) : null}

                {activeScreen.kind === "process-detail" ? (
                  <ProcessDetailFocusScreen
                    onRequestRerun={handleRerunProcess}
                    process={activeProcessDetail}
                    usesLiveSnapshot={activeProcessDetailUsesLiveSnapshot}
                  />
                ) : null}
              </Suspense>
            </section>
          </div>
        )}
      </div>
    </main>
  );
}

type MainQuickReleaseOverlayProps = {
  repository: RepositoryInspectionEntry;
  onResolve?: (value?: MainQuickReleaseOverlayResult | null) => void;
};

function MainQuickReleaseOverlay({
  repository,
  onResolve,
}: MainQuickReleaseOverlayProps) {
  const [draft, setDraft] = useState<MainQuickReleaseDraft>({
    releaseVersion: "",
    versionSource: "manual",
  });
  const [validationErrors, setValidationErrors] =
    useState<MainQuickReleaseValidationErrors>({});
  const [dispatchError, setDispatchError] = useState<string | null>(null);
  const [isQueueing, setIsQueueing] = useState(false);
  const isLocalWorkspace = isLocalWorkspaceSource(repository);

  const handleQueueRelease = async () => {
    if (!isLocalWorkspace || isQueueing) {
      return;
    }

    const nextValidationErrors = validateMainQuickReleaseDraft(draft);
    if (nextValidationErrors.releaseVersion) {
      setValidationErrors(nextValidationErrors);
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
        source_kind: "local_workspace",
        source_ref: null,
        local_path: repository.local_path ?? repository.repo_url,
        unity_executable_path_override: null,
      });

      onResolve?.({
        gitTag: release.git_tag,
        kind: "queued",
      });
    } catch (error) {
      setDispatchError(buildRuntimeActionErrorMessage(error, "start"));
      setIsQueueing(false);
    }
  };

  return (
    <FullScreenModal
      description={buildProjectSourceDisplay(repository)}
      dismissible={!isQueueing}
      footer={
        <div className="publish-destination-editor-modal__footer">
          <Button
            disabled={isQueueing}
            onClick={() => onResolve?.(null)}
            size="sm"
            variant="ghost"
          >
            Cancel
          </Button>
          {isLocalWorkspace ? (
            <Button
              disabled={isQueueing}
              leadingIcon="arrowUpRight"
              onClick={() => {
                void handleQueueRelease();
              }}
              size="sm"
              variant="secondary"
            >
              {isQueueing ? "Queueing..." : "Queue Local Release"}
            </Button>
          ) : (
            <Button
              leadingIcon="layout"
              onClick={() => onResolve?.({ kind: "open-project-list" })}
              size="sm"
              variant="secondary"
            >
              Open projects
            </Button>
          )}
        </div>
      }
      onResolve={onResolve}
      title={`Start release · ${repository.repository_name}`}
    >
      {isLocalWorkspace ? (
        <div className="project-detail-form-grid">
          <SelectField
            hint="Choose whether HGP should use a manual release label or detect it from project settings."
            label="Version source"
            onChange={(event) =>
              setDraft((current) => ({
                ...current,
                versionSource: event.currentTarget
                  .value as OnDemandReleaseVersionSource,
              }))
            }
            options={MAIN_QUICK_RELEASE_VERSION_SOURCE_OPTIONS}
            value={draft.versionSource}
          />
          <TextField
            data-overlay-autofocus
            disabled={draft.versionSource !== "manual"}
            error={validationErrors.releaseVersion}
            hint={
              draft.versionSource === "manual"
                ? "Use the release label that should identify this local snapshot."
                : "Detected from project settings when the release is queued."
            }
            label="Release version"
            onChange={(event) =>
              setDraft((current) => ({
                ...current,
                releaseVersion: event.currentTarget.value,
              }))
            }
            placeholder="v1.2.3"
            value={draft.releaseVersion}
          />
          {dispatchError ? (
            <p className="feed-banner feed-banner--error">{dispatchError}</p>
          ) : null}
        </div>
      ) : (
        <div className="feed-state">
          <p className="feed-state__title">
            Quick release from main is currently available for local workspace
            projects.
          </p>
          <p className="feed-state__copy">
            Open project list to continue with managed repository release flows.
          </p>
        </div>
      )}
    </FullScreenModal>
  );
}

async function waitForShellTransitionPhase() {
  if (prefersReducedMotion()) {
    return;
  }

  await waitForAnimationFrames(2);
}

async function waitForAnimationFrames(frameCount: number) {
  for (let frameIndex = 0; frameIndex < frameCount; frameIndex += 1) {
    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => {
        resolve();
      });
    });
  }
}

function prefersReducedMotion() {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

function buildInvokeErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return "The desktop shell could not refresh the process feed.";
}

function buildEventSubscriptionErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return "The desktop shell could not subscribe to runtime events.";
}

function buildWindowTransitionErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return "The desktop shell could not transition the current window.";
}

function buildWorkerInspectionErrorMessage(error: unknown): string {
  const message = readErrorMessage(error);

  if (message) {
    return message;
  }

  return "The desktop shell could not refresh the project worker inventory.";
}

function buildRuntimeActionMessage(action: RuntimeControlAction): string {
  switch (action) {
    case "start":
      return "Runtime start requested.";
    case "stop":
      return "Runtime stop requested.";
    case "restart":
      return "Runtime restart requested.";
  }
}

function buildRuntimeAutomationMessage(mode: RuntimeAutomationMode): string {
  return mode === "idle"
    ? "Automatic polling paused for the local host."
    : "Automatic polling resumed for the local host.";
}

function buildBulkInstantCheckMessage(
  queuedWorkers: ProjectWorkerEntry[],
): string {
  if (queuedWorkers.length === 1) {
    return `Instant check queued for ${queuedWorkers[0].repositoryName}.`;
  }

  return `Instant checks queued for ${queuedWorkers.length} projects.`;
}

function buildRuntimeActionErrorMessage(
  error: unknown,
  action: RuntimeControlAction,
): string {
  const message = readErrorMessage(error);

  if (message) {
    return message;
  }

  return `The desktop shell could not ${action} the runtime.`;
}

function buildRuntimeAutomationErrorMessage(
  error: unknown,
  mode: RuntimeAutomationMode,
): string {
  const message = readErrorMessage(error);

  if (message) {
    return message;
  }

  return mode === "idle"
    ? "The desktop shell could not pause automatic polling."
    : "The desktop shell could not resume automatic polling.";
}

function validateMainQuickReleaseDraft(
  draft: MainQuickReleaseDraft,
): MainQuickReleaseValidationErrors {
  if (draft.versionSource !== "manual") {
    return {};
  }

  return draft.releaseVersion.trim()
    ? {}
    : {
        releaseVersion:
          "Release version is required for manual local dispatch.",
      };
}

function buildRepositoryInstantCheckErrorMessage(
  error: unknown,
  repositoryName: string,
): string {
  const message = readErrorMessage(error);

  if (message) {
    return message;
  }

  return `The desktop shell could not queue an instant check for ${repositoryName}.`;
}

function buildBulkRepositoryInstantCheckErrorMessage(
  error: unknown,
  repositoryName: string | null,
  queuedProjectCount: number,
) {
  const message = readErrorMessage(error);

  if (message) {
    return message;
  }

  if (repositoryName && queuedProjectCount > 0) {
    return `The desktop shell could not continue the bulk instant check while queueing ${repositoryName} after ${queuedProjectCount} ${queuedProjectCount === 1 ? "project" : "projects"}.`;
  }

  if (repositoryName) {
    return `The desktop shell could not queue a bulk instant check for ${repositoryName}.`;
  }

  return "The desktop shell could not queue the selected bulk instant checks.";
}

function buildProjectWorkerSelectionItems(
  projectWorkers: ProjectWorkerEntry[],
): SelectListItem[] {
  return projectWorkers.map((projectWorker) => ({
    id: String(projectWorker.repositoryId),
    label: projectWorker.repositoryName,
    subtitle: `${projectWorker.buildTargets.length} ${projectWorker.buildTargets.length === 1 ? "target" : "targets"} • poll ${projectWorker.pollingIntervalSeconds}s`,
  }));
}

function buildBulkInstantCheckConfirmationMessage(
  projectWorkers: ProjectWorkerEntry[],
) {
  const workerNames = projectWorkers.map(
    (projectWorker) => projectWorker.repositoryName,
  );

  if (workerNames.length === 1) {
    return `Queue an immediate repository check for ${workerNames[0]}.`;
  }

  return `Queue immediate repository checks for ${workerNames.join(", ")}.`;
}

function readErrorMessage(error: unknown): string | null {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return null;
}

function resolveWorkerStatusSummary(
  t: Translate,
  snapshot: WorkerStatusSnapshot,
): WorkerStatusSummary {
  const projectWorkers = collectProjectWorkers(snapshot.repositories);

  if (snapshot.inspectionError && !snapshot.inspectionAvailable) {
    return {
      tone: "warning",
      label: t(
        "app.main.worker_status.inventory_unavailable",
        "Project worker inventory is unavailable until repository inspection succeeds again.",
      ),
      animated: false,
    };
  }

  if (snapshot.inspectionStale) {
    return {
      tone: "warning",
      label: t(
        "app.main.worker_status.inventory_stale",
        "Project worker inventory may be stale for {{projectCount}} while repository inspection recovers.",
        {
          projectCount: formatProjectCount(t, projectWorkers.length),
        },
      ),
      animated: false,
    };
  }

  if (!snapshot.inspectionAvailable) {
    return {
      tone: "idle",
      label: t(
        "app.main.worker_status.loading",
        "Project worker status is unavailable while the shell loads repository inspection.",
      ),
      animated: false,
    };
  }

  if (projectWorkers.length === 0) {
    return {
      tone: "idle",
      label: t(
        "app.main.worker_status.none_configured",
        "No active project workers are configured.",
      ),
      animated: false,
    };
  }

  if (snapshot.runtimeStatus === null) {
    return {
      tone: "idle",
      label: t(
        "app.main.worker_status.runtime_unavailable",
        "Project workers are down for {{projectCount}} because runtime health is unavailable.",
        {
          projectCount: formatProjectCount(t, projectWorkers.length),
        },
      ),
      animated: false,
    };
  }

  if (snapshot.runtimeStatus === "unhealthy") {
    return {
      tone: "warning",
      label: t(
        "app.main.worker_status.runtime_unhealthy",
        "Worker warning: the runtime is unhealthy for {{projectCount}}.",
        {
          projectCount: formatProjectCount(t, projectWorkers.length),
        },
      ),
      animated: true,
    };
  }

  if (snapshot.runtimeStatus !== "healthy") {
    return {
      tone: "idle",
      label: t(
        "app.main.worker_status.runtime_down",
        "Project workers are down for {{projectCount}} while the runtime is {{runtimeStatus}}.",
        {
          projectCount: formatProjectCount(t, projectWorkers.length),
          runtimeStatus: formatRuntimeStatus(t, snapshot.runtimeStatus),
        },
      ),
      animated: false,
    };
  }

  if (snapshot.automationMode === "idle") {
    return {
      tone: "idle",
      label: t(
        "app.main.worker_status.polling_paused",
        "Automatic polling paused for {{projectCount}}.",
        {
          projectCount: formatProjectCount(t, projectWorkers.length),
        },
      ),
      animated: false,
    };
  }

  const failingTargets = collectRelevantBuildTargets(projectWorkers).filter(
    (buildTarget) => buildTarget.diagnosticStatus !== "ready",
  );

  if (failingTargets.length > 0) {
    return {
      tone: "warning",
      label: t(
        "app.main.worker_status.build_target_warning",
        "Build target warning across {{projectCount}}: {{buildTargetCount}} requiring attention.",
        {
          buildTargetCount: formatBuildTargetCount(t, failingTargets.length),
          projectCount: formatProjectCount(t, projectWorkers.length),
        },
      ),
      animated: true,
    };
  }

  return {
    tone: "success",
    label: t(
      "app.main.worker_status.active",
      "Project workers active for {{projectCount}}.",
      {
        projectCount: formatProjectCount(t, projectWorkers.length),
      },
    ),
    animated: true,
  };
}

type RelevantBuildTarget = {
  buildTargetId: number;
  diagnosticStatus: string;
};

function collectRelevantBuildTargets(
  projectWorkers: ProjectWorkerEntry[],
): RelevantBuildTarget[] {
  return projectWorkers.flatMap((projectWorker) =>
    projectWorker.buildTargets.map((buildTarget) => ({
      buildTargetId: buildTarget.buildTargetId,
      diagnosticStatus: buildTarget.diagnosticStatus,
    })),
  );
}

function collectProjectWorkers(
  repositories: RepositoryInspectionEntry[],
): ProjectWorkerEntry[] {
  return repositories
    .filter((repository) => repository.enabled)
    .map((repository) => ({
      pollingIntervalSeconds: repository.polling_interval_seconds,
      repositoryId: repository.repository_id,
      repositoryName: repository.repository_name,
      buildTargets: repository.build_targets
        .filter((target) => target.enabled)
        .map((target) => ({
          buildTargetId: target.build_target_id,
          diagnosticMessage: target.diagnostic_message,
          diagnosticStatus: target.diagnostic_status.trim().toLowerCase(),
          name: target.target_name,
          unityTargetPlatform: target.unity_target_platform,
        })),
    }))
    .filter((projectWorker) => projectWorker.buildTargets.length > 0);
}

function formatProjectCount(t: Translate, projectCount: number) {
  return projectCount === 1
    ? t("app.count.active_project.one", "1 active project")
    : t("app.count.active_project.other", "{{count}} active projects", {
        count: projectCount,
      });
}

function formatBuildTargetCount(t: Translate, buildTargetCount: number) {
  return buildTargetCount === 1
    ? t("app.count.build_target.one", "1 build target")
    : t("app.count.build_target.other", "{{count}} build targets", {
        count: buildTargetCount,
      });
}

function buildWorkerStatusDescription(
  t: Translate,
  snapshot: WorkerStatusSnapshot,
  projectWorkers: ProjectWorkerEntry[],
) {
  if (snapshot.inspectionError && !snapshot.inspectionAvailable) {
    return snapshot.inspectionError;
  }

  if (snapshot.inspectionStale) {
    return t(
      "app.main.worker_description.stale",
      "Showing the last known worker snapshot while repository inspection recovers. {{activeWorkers}}",
      {
        activeWorkers:
          projectWorkers.length > 0
            ? buildActiveWorkersDescription(t, projectWorkers)
            : "",
      },
    ).trim();
  }

  if (!snapshot.inspectionAvailable) {
    return t(
      "app.main.worker_description.loading",
      "Loading active workers...",
    );
  }

  if (projectWorkers.length === 0) {
    return t(
      "app.main.worker_description.none",
      "No active workers configured.",
    );
  }

  if (snapshot.automationMode === "idle") {
    return t(
      "app.main.worker_description.polling_paused",
      "Automatic polling is paused. Manual instant checks remain available. {{activeWorkers}}",
      {
        activeWorkers: buildActiveWorkersDescription(t, projectWorkers),
      },
    );
  }

  return buildActiveWorkersDescription(t, projectWorkers);
}

function buildActiveWorkersDescription(
  t: Translate,
  projectWorkers: ProjectWorkerEntry[],
) {
  return `${t("app.main.worker_description.active_prefix", "Active workers:")} ${projectWorkers
    .map((projectWorker) => {
      const buildTargetNames = projectWorker.buildTargets
        .map((buildTarget) => buildTarget.name)
        .join(", ");

      return `${projectWorker.repositoryName} (${buildTargetNames})`;
    })
    .join(" · ")}`;
}

function buildWorkerStatusTooltip(
  t: Translate,
  automationMode: RuntimeAutomationMode | null,
  projectWorkers: ProjectWorkerEntry[],
) {
  if (projectWorkers.length === 0) {
    return automationMode === "idle"
      ? t("app.main.worker_tooltip.paused", "Automatic polling paused")
      : undefined;
  }

  const repositoryNames = projectWorkers
    .map((projectWorker) => projectWorker.repositoryName)
    .join(", ");

  if (automationMode === "idle") {
    return (
      t("app.main.worker_tooltip.paused", "Automatic polling paused") +
      ` · ${repositoryNames}`
    );
  }

  return repositoryNames;
}

function resolveAutomationToggleLabel(
  t: Translate,
  automationMode: RuntimeAutomationMode | null,
  pendingAutomationMode: RuntimeAutomationMode | null,
) {
  if (pendingAutomationMode === "idle") {
    return t("app.main.automation_toggle.pausing", "Pausing...");
  }

  if (pendingAutomationMode === "active") {
    return t("app.main.automation_toggle.resuming", "Resuming...");
  }

  if (automationMode === "idle") {
    return t("app.main.automation_toggle.resume", "Resume polling");
  }

  if (automationMode === "active") {
    return t("app.main.automation_toggle.pause", "Pause polling");
  }

  return t("app.main.automation_toggle.status", "Polling status...");
}

function formatRuntimeStatus(t: Translate, status: RuntimeHealthStatus) {
  switch (status) {
    case "bootstrapping":
      return t("app.runtime_status.bootstrapping", "bootstrapping");
    case "healthy":
      return t("app.runtime_status.healthy", "healthy");
    case "shutting_down":
      return t("app.runtime_status.shutting_down", "shutting down");
    case "stopped":
      return t("app.runtime_status.stopped", "stopped");
    case "unhealthy":
      return t("app.runtime_status.unhealthy", "unhealthy");
  }
}

function resolveWindowTitle(
  t: Translate,
  productName: string,
  activeScreen: AppScreen,
  activeProjectTitle: string | null,
) {
  switch (activeScreen.kind) {
    case "main":
      return productName;
    case "create-project":
      return `${productName} · ${t("app.window.title.create_project", "Create Project")}`;
    case "auth-providers":
      return `${productName} · ${t("app.window.title.auth_providers", "Logins")}`;
    case "settings":
      return `${productName} · ${t("app.window.title.settings", "Settings")}`;
    case "project-list":
      return `${productName} · ${t("app.window.title.projects", "Projects")}`;
    case "project-workers":
      return `${productName} · ${t("app.window.title.project_workers", "Project Workers")}`;
    case "project-detail":
      return activeProjectTitle?.trim()
        ? `${productName} · ${activeProjectTitle.trim()}`
        : `${productName} · ${t("app.window.title.project_number", "Project #{{repositoryId}}", { repositoryId: activeScreen.repositoryId })}`;
    case "process-detail":
      return `${productName} · ${t("app.window.title.process_number", "Process #{{releaseRunId}}", { releaseRunId: activeScreen.process.release_run_id })}`;
  }
}

function resolveFocusBackLabel(t: Translate, activeScreen: AppScreen) {
  if (
    activeScreen.kind === "project-detail" &&
    activeScreen.returnTo === "project-list"
  ) {
    return t("app.back.project_list", "Back to project list");
  }

  if (
    activeScreen.kind === "auth-providers" &&
    activeScreen.returnTo === "create-project"
  ) {
    return t("app.back.project_creation", "Back to project creation");
  }

  if (
    activeScreen.kind === "auth-providers" &&
    activeScreen.returnTo === "settings"
  ) {
    return t("app.back.settings", "Back to settings");
  }

  return t("app.back.main", "Back to main screen");
}

function resolveFocusScreenShellClassName(activeScreen: AppScreen) {
  switch (activeScreen.kind) {
    case "create-project":
      return "focus-screen-shell focus-screen-shell--wizard";
    case "auth-providers":
      return "focus-screen-shell focus-screen-shell--auth-providers";
    case "project-workers":
      return "focus-screen-shell focus-screen-shell--project-workers";
    case "project-list":
      return "focus-screen-shell focus-screen-shell--project-list";
    case "project-detail":
      return "focus-screen-shell focus-screen-shell--project-detail";
    case "process-detail":
      return "focus-screen-shell focus-screen-shell--process-detail";
    default:
      return "focus-screen-shell";
  }
}

export default App;
