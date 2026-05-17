import {
  startTransition,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { Button, IconButton } from "./components/Button";
import { AuthProvidersFocusScreen } from "./components/AuthProvidersFocusScreen";
import { CreateProjectWizard } from "./components/CreateProjectWizard";
import { ProcessFeedItem } from "./components/ProcessFeedItem";
import { ProcessDetailFocusScreen } from "./components/ProcessDetailFocusScreen";
import { ProjectsFocusScreen } from "./components/ProjectsFocusScreen";
import { RepositoryProjectDetail } from "./components/RepositoryProjectDetail";
import {
  WorkerStatusIndicator,
  type WorkerStatusTone,
} from "./components/WorkerStatusIndicator";
import { type ProcessFeedRecord } from "./components/processFeedPresentation";
import {
  ProjectWorkersFocusScreen,
  type ProjectWorkerEntry,
  type RuntimeControlAction,
} from "./components/ProjectWorkersFocusScreen";
import {
  loadRepositoryInspection,
  type RepositoryInspectionEntry,
} from "./services/projects";
import {
  subscribeToProcessFeedEvents,
  type ProcessFeedRuntimeEvent,
} from "./services/processFeed";
import {
  loadRuntimeHealth,
  requestRepositoryInstantCheck,
  restartRuntime,
  startRuntime,
  stopRuntime,
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
  repositories: RepositoryInspectionEntry[];
  inspectionAvailable: boolean;
  runtimeStatus: RuntimeHealthStatus | null;
};

type AppScreen =
  | { kind: "main" }
  | { kind: "create-project" }
  | { kind: "auth-providers"; returnTo: "main" | "create-project" }
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
const PRODUCT_NAME = "Handy Games Publisher";
const WORKER_STATUS_REFRESH_INTERVAL_MILLIS = 5_000;
const WORKER_TOOLTIP_ID = "worker-status-tooltip";
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
  repositories: [],
  inspectionAvailable: false,
  runtimeStatus: null,
};

function App() {
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
  const [isWorkerTooltipOpen, setIsWorkerTooltipOpen] = useState(false);
  const [workerActionError, setWorkerActionError] = useState<string | null>(
    null,
  );
  const [workerActionMessage, setWorkerActionMessage] = useState<string | null>(
    null,
  );
  const [pendingRuntimeAction, setPendingRuntimeAction] =
    useState<RuntimeControlAction | null>(null);
  const [pendingInstantCheckRepositoryId, setPendingInstantCheckRepositoryId] =
    useState<number | null>(null);
  const latestRequestIdRef = useRef(0);
  const latestWorkerStatusRequestIdRef = useRef(0);
  const isNavigatingRef = useRef(false);
  const workerStatus = resolveWorkerStatusSummary(workerSnapshot);
  const projectWorkers = collectProjectWorkers(workerSnapshot.repositories);
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

  const loadWorkerStatus = useEffectEvent(async () => {
    const requestId = latestWorkerStatusRequestIdRef.current + 1;
    latestWorkerStatusRequestIdRef.current = requestId;

    const [inspectionResult, healthResult] = await Promise.allSettled([
      loadRepositoryInspection(),
      loadRuntimeHealth(),
    ]);

    if (requestId !== latestWorkerStatusRequestIdRef.current) {
      return;
    }

    const nextSnapshot: WorkerStatusSnapshot = {
      repositories:
        inspectionResult.status === "fulfilled"
          ? inspectionResult.value.repositories
          : [],
      inspectionAvailable: inspectionResult.status === "fulfilled",
      runtimeStatus:
        healthResult.status === "fulfilled" ? healthResult.value.status : null,
    };

    startTransition(() => {
      setWorkerSnapshot(nextSnapshot);
    });
  });

  useEffect(() => {
    void loadWorkerStatus();

    const intervalId = window.setInterval(() => {
      void loadWorkerStatus();
    }, WORKER_STATUS_REFRESH_INTERVAL_MILLIS);

    return () => {
      window.clearInterval(intervalId);
    };
  }, []);

  const handleProcessFeedEvent = useEffectEvent(
    (event: ProcessFeedRuntimeEvent) => {
      if (event.topic === "automation.release_queued" && page !== 1) {
        startTransition(() => {
          setPage(1);
        });
        void loadWorkerStatus();
        return;
      }

      void loadProcessFeed(page, "event");
      void loadWorkerStatus();
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

  const transitionToScreen = useEffectEvent(async (nextScreen: AppScreen) => {
    if (isNavigatingRef.current) {
      return;
    }

    isNavigatingRef.current = true;
    setTransitionError(null);
    setIsScreenBlank(true);

    try {
      await waitForBlankPaint();
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

  const handleCloseMainWindow = useEffectEvent(async () => {
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

  const handleOpenAuthProvidersFromWizard = useEffectEvent(() => {
    void transitionToScreen({
      kind: "auth-providers",
      returnTo: "create-project",
    });
  });

  const handleOpenSettings = useEffectEvent(() => {
    void transitionToScreen({ kind: "settings" });
  });

  const handleOpenProjects = useEffectEvent(() => {
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
    setIsWorkerTooltipOpen(false);
    setWorkerActionError(null);
    setWorkerActionMessage(null);
    void loadWorkerStatus();
    void transitionToScreen({ kind: "project-workers" });
  });

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
    void runRuntimeLifecycleAction("stop");
  });

  const handleRestartRuntime = useEffectEvent(() => {
    void runRuntimeLifecycleAction("restart");
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

    void transitionToScreen({ kind: "main" });
  });

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
            {resolveWindowTitle(activeScreen, activeProjectTitle)}
          </span>
        </div>
        <div className="window-titlebar__actions">
          <IconButton
            aria-pressed={isMainWindowPinned}
            className="window-titlebar__action window-titlebar__action--pin"
            icon={isMainWindowPinned ? "unpin" : "pin"}
            label={isMainWindowPinned ? "Desafixar janela" : "Fixar janela"}
            onClick={handleToggleMainWindowPinned}
            size="sm"
            variant="ghost"
          />
          <IconButton
            className="window-titlebar__action window-titlebar__action--close"
            icon="close"
            label="Fechar janela"
            onClick={handleCloseMainWindow}
            size="sm"
            variant="ghost"
          />
        </div>
      </header>

      <div className="app-shell__content">
        {isScreenBlank ? null : activeScreen.kind === "main" ? (
          <div className="home-frame">
            <section className="action-bar" aria-label="Primary actions">
              <div className="action-bar__leading">
                <div
                  className="worker-status-shell"
                  onBlurCapture={() => {
                    setIsWorkerTooltipOpen(false);
                  }}
                  onFocusCapture={() => {
                    setIsWorkerTooltipOpen(true);
                  }}
                  onMouseEnter={() => {
                    setIsWorkerTooltipOpen(true);
                  }}
                  onMouseLeave={() => {
                    setIsWorkerTooltipOpen(false);
                  }}
                >
                  <WorkerStatusIndicator
                    animated={workerStatus.animated}
                    aria-controls={WORKER_TOOLTIP_ID}
                    expanded={isWorkerTooltipOpen}
                    label={workerStatus.label}
                    onClick={handleOpenProjectWorkers}
                    tone={workerStatus.tone}
                  />

                  {isWorkerTooltipOpen ? (
                    <section
                      className="worker-status-tooltip"
                      id={WORKER_TOOLTIP_ID}
                      role="tooltip"
                    >
                      <header className="worker-status-tooltip__header">
                        <h2 className="worker-status-tooltip__title">
                          Project Workers
                        </h2>
                      </header>

                      <div className="worker-status-tooltip__content">
                        {!workerSnapshot.inspectionAvailable ? (
                          <p className="worker-status-tooltip__empty">
                            Loading project worker inventory...
                          </p>
                        ) : null}

                        {workerSnapshot.inspectionAvailable &&
                        projectWorkers.length === 0 ? (
                          <p className="worker-status-tooltip__empty">
                            No active project workers configured.
                          </p>
                        ) : null}

                        {workerSnapshot.inspectionAvailable &&
                        projectWorkers.length > 0 ? (
                          <ul className="worker-status-tooltip__list">
                            {projectWorkers.map((projectWorker) => (
                              <li
                                className="worker-status-tooltip__item"
                                key={projectWorker.repositoryId}
                              >
                                <span className="worker-status-tooltip__project-name">
                                  {projectWorker.repositoryName}
                                </span>
                              </li>
                            ))}
                          </ul>
                        ) : null}
                      </div>
                    </section>
                  ) : null}
                </div>
              </div>

              <div className="action-bar__actions">
                <IconButton
                  icon="layout"
                  label="Projetos"
                  onClick={handleOpenProjects}
                  size="sm"
                  variant="secondary"
                />
                <IconButton
                  icon="plus"
                  label="Criar novo projeto"
                  onClick={handleOpenCreateProject}
                  size="sm"
                  variant="primary"
                />
              </div>
            </section>

            <section className="process-feed-shell" aria-label="Process list">
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
                  <p className="feed-state__title">Loading process feed...</p>
                  <p className="feed-state__copy">
                    The shell is querying the runtime for recent build and
                    publishing activity.
                  </p>
                </div>
              ) : null}

              {!isLoadingFeed && processPage.items.length === 0 ? (
                <div className="feed-state">
                  <p className="feed-state__title">
                    No processes recorded yet.
                  </p>
                  <p className="feed-state__copy">
                    New build or publishing runs will appear here as soon as the
                    runtime creates them.
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
                      Previous
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
                      Next
                    </Button>
                  </div>
                </footer>
              ) : null}
            </section>

            <section
              className="action-bar action-bar--home-bottom"
              aria-label="Secondary actions"
            >
              <div className="action-bar__actions">
                <IconButton
                  icon="key"
                  label="Auth"
                  onClick={handleOpenAuthProviders}
                  size="sm"
                  variant="ghost"
                />
                <IconButton
                  icon="settings"
                  label="Settings"
                  onClick={handleOpenSettings}
                  size="sm"
                  variant="ghost"
                />
              </div>
            </section>
          </div>
        ) : (
          <div className="focus-frame">
            <section
              className="action-bar action-bar--focus"
              aria-label="Process detail actions"
            >
              <div className="action-bar__actions action-bar__actions--leading">
                <IconButton
                  icon="arrowLeft"
                  label={resolveFocusBackLabel(activeScreen)}
                  onClick={handleReturnFromFocus}
                  size="sm"
                  variant="ghost"
                />
              </div>
            </section>

            <section
              className={
                activeScreen.kind === "create-project"
                  ? "focus-screen-shell focus-screen-shell--wizard"
                  : activeScreen.kind === "auth-providers"
                    ? "focus-screen-shell focus-screen-shell--auth-providers"
                    : activeScreen.kind === "project-workers"
                      ? "focus-screen-shell focus-screen-shell--project-workers"
                      : activeScreen.kind === "project-list"
                        ? "focus-screen-shell focus-screen-shell--project-list"
                        : activeScreen.kind === "project-detail"
                          ? "focus-screen-shell focus-screen-shell--project-detail"
                          : activeScreen.kind === "process-detail"
                            ? "focus-screen-shell focus-screen-shell--process-detail"
                            : "focus-screen-shell"
              }
              aria-label="Focus screen"
            >
              {transitionError ? (
                <p className="feed-banner feed-banner--error">
                  {transitionError}
                </p>
              ) : null}

              {activeScreen.kind === "create-project" ? (
                <CreateProjectWizard
                  onCreated={handleProjectCreated}
                  onManageAuth={handleOpenAuthProvidersFromWizard}
                />
              ) : null}

              {activeScreen.kind === "auth-providers" ? (
                <AuthProvidersFocusScreen />
              ) : null}

              {activeScreen.kind === "settings" ? (
                <p className="focus-screen-shell__title">Settings</p>
              ) : null}

              {activeScreen.kind === "project-list" ? (
                <ProjectsFocusScreen
                  highlightedRepositoryId={activeScreen.highlightedRepositoryId}
                  onOpenProject={handleOpenProjectDetail}
                />
              ) : null}

              {activeScreen.kind === "project-workers" ? (
                <ProjectWorkersFocusScreen
                  actionError={workerActionError}
                  actionMessage={workerActionMessage}
                  inspectionAvailable={workerSnapshot.inspectionAvailable}
                  onInstantCheck={handleRepositoryInstantCheck}
                  onRestartRuntime={handleRestartRuntime}
                  onStartRuntime={handleStartRuntime}
                  onStopRuntime={handleStopRuntime}
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
                  repositoryId={activeScreen.repositoryId}
                />
              ) : null}

              {activeScreen.kind === "process-detail" ? (
                <ProcessDetailFocusScreen
                  process={activeProcessDetail}
                  usesLiveSnapshot={activeProcessDetailUsesLiveSnapshot}
                />
              ) : null}
            </section>
          </div>
        )}
      </div>
    </main>
  );
}

async function waitForBlankPaint() {
  await new Promise<void>((resolve) => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        resolve();
      });
    });
  });
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
  snapshot: WorkerStatusSnapshot,
): WorkerStatusSummary {
  const projectWorkers = collectProjectWorkers(snapshot.repositories);

  if (!snapshot.inspectionAvailable) {
    return {
      tone: "idle",
      label:
        "Project worker status is unavailable while the shell loads repository inspection.",
      animated: false,
    };
  }

  if (projectWorkers.length === 0) {
    return {
      tone: "idle",
      label: "No active project workers are configured.",
      animated: false,
    };
  }

  if (snapshot.runtimeStatus === null) {
    return {
      tone: "idle",
      label: `Project workers are down for ${formatProjectCount(projectWorkers.length)} because runtime health is unavailable.`,
      animated: false,
    };
  }

  if (snapshot.runtimeStatus === "unhealthy") {
    return {
      tone: "warning",
      label: `Worker warning: the runtime is unhealthy for ${formatProjectCount(projectWorkers.length)}.`,
      animated: true,
    };
  }

  if (snapshot.runtimeStatus !== "healthy") {
    return {
      tone: "idle",
      label: `Project workers are down for ${formatProjectCount(projectWorkers.length)} while the runtime is ${formatRuntimeStatus(snapshot.runtimeStatus)}.`,
      animated: false,
    };
  }

  const failingTargets = collectRelevantBuildTargets(projectWorkers).filter(
    (buildTarget) => buildTarget.diagnosticStatus !== "ready",
  );

  if (failingTargets.length > 0) {
    return {
      tone: "warning",
      label: `Build target warning: ${formatBuildTargetCount(failingTargets.length)} ${failingTargets.length === 1 ? "needs" : "need"} attention across ${formatProjectCount(projectWorkers.length)}.`,
      animated: true,
    };
  }

  return {
    tone: "success",
    label: `Project workers active for ${formatProjectCount(projectWorkers.length)}.`,
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

function formatProjectCount(projectCount: number) {
  return `${projectCount} active project${projectCount === 1 ? "" : "s"}`;
}

function formatBuildTargetCount(buildTargetCount: number) {
  return `${buildTargetCount} build target${buildTargetCount === 1 ? "" : "s"}`;
}

function formatRuntimeStatus(status: RuntimeHealthStatus) {
  return status.replace(/_/g, " ");
}

function resolveWindowTitle(
  activeScreen: AppScreen,
  activeProjectTitle: string | null,
) {
  switch (activeScreen.kind) {
    case "main":
      return PRODUCT_NAME;
    case "create-project":
      return `${PRODUCT_NAME} · Create Project`;
    case "auth-providers":
      return `${PRODUCT_NAME} · Logins`;
    case "settings":
      return `${PRODUCT_NAME} · Settings`;
    case "project-list":
      return `${PRODUCT_NAME} · Projects`;
    case "project-workers":
      return `${PRODUCT_NAME} · Project Workers`;
    case "project-detail":
      return activeProjectTitle?.trim()
        ? `${PRODUCT_NAME} · ${activeProjectTitle.trim()}`
        : `${PRODUCT_NAME} · Project #${activeScreen.repositoryId}`;
    case "process-detail":
      return `${PRODUCT_NAME} · Process #${activeScreen.process.release_run_id}`;
  }
}

function resolveFocusBackLabel(activeScreen: AppScreen) {
  if (
    activeScreen.kind === "project-detail" &&
    activeScreen.returnTo === "project-list"
  ) {
    return "Voltar para a lista de projetos";
  }

  if (
    activeScreen.kind === "auth-providers" &&
    activeScreen.returnTo === "create-project"
  ) {
    return "Voltar para a criação do projeto";
  }

  return "Voltar para a tela principal";
}

export default App;
