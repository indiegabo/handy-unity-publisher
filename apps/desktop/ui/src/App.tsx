import {
  startTransition,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";
import { invoke } from "@tauri-apps/api/core";

import { Button, IconButton } from "./components/Button";
import {
  ProcessFeedItem,
  type ProcessFeedRecord,
} from "./components/ProcessFeedItem";
import { CreateProjectWizard } from "./components/CreateProjectWizard";
import { RepositoryProjectDetail } from "./components/RepositoryProjectDetail";
import {
  subscribeToProcessFeedEvents,
  type ProcessFeedRuntimeEvent,
} from "./services/processFeed";

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

type AppScreen =
  | { kind: "main" }
  | { kind: "create-project" }
  | { kind: "project-detail"; repositoryId: number }
  | { kind: "process-detail"; processId: number };

const PROCESS_FEED_PAGE_SIZE = 5;
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

function App() {
  const [page, setPage] = useState(1);
  const [activeScreen, setActiveScreen] = useState<AppScreen>({ kind: "main" });
  const [isScreenBlank, setIsScreenBlank] = useState(false);
  const [transitionError, setTransitionError] = useState<string | null>(null);
  const [processPage, setProcessPage] = useState<ProcessFeedPage>(
    EMPTY_PROCESS_FEED_PAGE,
  );
  const [isLoadingFeed, setIsLoadingFeed] = useState(true);
  const [, setIsRefreshingFeed] = useState(false);
  const [feedError, setFeedError] = useState<string | null>(null);
  const latestRequestIdRef = useRef(0);
  const isNavigatingRef = useRef(false);

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

  useEffect(() => {
    void loadProcessFeed(page, "page");
  }, [page]);

  const handleProcessFeedEvent = useEffectEvent(
    (event: ProcessFeedRuntimeEvent) => {
      if (event.topic === "automation.release_queued" && page !== 1) {
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
        processId: process.release_run_id,
      });
    },
  );

  const handleReturnToMain = useEffectEvent(() => {
    void transitionToScreen({ kind: "main" });
  });

  const handleOpenCreateProject = useEffectEvent(() => {
    void transitionToScreen({ kind: "create-project" });
  });

  const handleProjectCreated = useEffectEvent((repositoryId: number) => {
    startTransition(() => {
      setActiveScreen({
        kind: "project-detail",
        repositoryId,
      });
    });
  });

  return (
    <main className="app-shell">
      {isScreenBlank ? null : activeScreen.kind === "main" ? (
        <div className="home-frame">
          <section className="action-bar" aria-label="Primary actions">
            <div className="action-bar__actions">
              <IconButton
                icon="layout"
                label="Projetos"
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
                <p className="feed-state__title">No processes recorded yet.</p>
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
                label="Voltar para a tela principal"
                onClick={handleReturnToMain}
                size="sm"
                variant="ghost"
              />
            </div>
          </section>

          <section
            className={
              activeScreen.kind === "create-project"
                ? "focus-screen-shell focus-screen-shell--wizard"
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
              <CreateProjectWizard onCreated={handleProjectCreated} />
            ) : null}

            {activeScreen.kind === "project-detail" ? (
              <RepositoryProjectDetail
                repositoryId={activeScreen.repositoryId}
              />
            ) : null}

            {activeScreen.kind === "process-detail" ? (
              <p className="focus-screen-shell__title">
                {`Detalhe do processo #${activeScreen.processId}`}
              </p>
            ) : null}
          </section>
        </div>
      )}
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

export default App;
