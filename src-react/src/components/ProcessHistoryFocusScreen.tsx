import {
  startTransition,
  useDeferredValue,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";

import { useLocalization } from "../LocalizationProvider";
import {
  loadProcessFeed,
  subscribeToProcessFeedEvents,
  type ProcessFeedPage,
  type ProcessFeedStatusFilter,
} from "../services/processFeed";
import { Button } from "./Button";
import { SelectField, TextField, type SelectOption } from "./Field";
import { ProcessFeedItem } from "./ProcessFeedItem";
import ScreenScaffold from "./ScreenScaffold";
import {
  formatLocalizedProcessFeedStatusLabel,
  type ProcessFeedRecord,
} from "./processFeedPresentation";

type ProcessHistoryFocusScreenProps = {
  onOpenDetail: (process: ProcessFeedRecord) => void;
  onRequestCancel?: (process: ProcessFeedRecord) => Promise<void>;
};

type LoadReason = "page" | "refresh" | "event";

const PROCESS_HISTORY_PAGE_SIZE = 12;
const EMPTY_PROCESS_FEED_PAGE: ProcessFeedPage = {
  generated_at: "",
  has_next_page: false,
  has_previous_page: false,
  items: [],
  page: 1,
  page_size: PROCESS_HISTORY_PAGE_SIZE,
  total_items: 0,
  total_pages: 0,
};

export function ProcessHistoryFocusScreen({
  onOpenDetail,
  onRequestCancel,
}: ProcessHistoryFocusScreenProps) {
  const { t } = useLocalization();
  const [page, setPage] = useState(1);
  const [processPage, setProcessPage] = useState<ProcessFeedPage>(
    EMPTY_PROCESS_FEED_PAGE,
  );
  const [isLoading, setIsLoading] = useState(true);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const deferredQuery = useDeferredValue(query);
  const [status, setStatus] = useState<ProcessFeedStatusFilter>("all");
  const [pendingCancelReleaseRunId, setPendingCancelReleaseRunId] = useState<
    number | null
  >(null);
  const latestRequestIdRef = useRef(0);

  const statusOptions: SelectOption[] = [
    {
      label: t("process_history.filters.status.all", "All statuses"),
      value: "all",
    },
    {
      label: formatLocalizedProcessFeedStatusLabel(t, "queued"),
      value: "queued",
    },
    {
      label: formatLocalizedProcessFeedStatusLabel(t, "running"),
      value: "running",
    },
    {
      label: formatLocalizedProcessFeedStatusLabel(t, "on_hold"),
      value: "on_hold",
    },
    {
      label: formatLocalizedProcessFeedStatusLabel(t, "succeeded"),
      value: "succeeded",
    },
    {
      label: formatLocalizedProcessFeedStatusLabel(t, "failed"),
      value: "failed",
    },
    {
      label: formatLocalizedProcessFeedStatusLabel(t, "canceled"),
      value: "canceled",
    },
  ];

  const loadHistory = useEffectEvent(
    async (pageToLoad: number, reason: LoadReason) => {
      const requestId = latestRequestIdRef.current + 1;
      latestRequestIdRef.current = requestId;

      if (reason === "page" && processPage.items.length === 0) {
        setIsLoading(true);
      } else {
        setIsRefreshing(true);
      }

      try {
        const response = await loadProcessFeed({
          page: pageToLoad,
          pageSize: PROCESS_HISTORY_PAGE_SIZE,
          query: deferredQuery,
          scope: "all",
          status,
        });

        if (requestId !== latestRequestIdRef.current) {
          return;
        }

        startTransition(() => {
          setProcessPage(response);
          setPage(response.page);
          setIsLoading(false);
          setIsRefreshing(false);
          setError(null);
        });
      } catch (loadError) {
        if (requestId !== latestRequestIdRef.current) {
          return;
        }

        startTransition(() => {
          setIsLoading(false);
          setIsRefreshing(false);
          setError(buildProcessHistoryErrorMessage(t, loadError));
        });
      }
    },
  );

  const handleCancelProcess = useEffectEvent(
    async (process: ProcessFeedRecord) => {
      if (!onRequestCancel) {
        return;
      }

      startTransition(() => {
        setPendingCancelReleaseRunId(process.release_run_id);
      });

      try {
        await onRequestCancel(process);
        await loadHistory(page, "event");
      } finally {
        startTransition(() => {
          setPendingCancelReleaseRunId((current) =>
            current === process.release_run_id ? null : current,
          );
        });
      }
    },
  );

  useEffect(() => {
    void loadHistory(page, "page");
  }, [deferredQuery, page, status]);

  useEffect(() => {
    let disposed = false;
    let unsubscribe: (() => void) | undefined;

    void subscribeToProcessFeedEvents(() => {
      if (disposed) {
        return;
      }

      void loadHistory(page, "event");
    })
      .then((dispose) => {
        if (disposed) {
          dispose();
          return;
        }

        unsubscribe = dispose;
      })
      .catch((subscriptionError: unknown) => {
        if (disposed) {
          return;
        }

        setError(buildProcessHistoryErrorMessage(t, subscriptionError));
      });

    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, []);

  const hasFilters = deferredQuery.trim().length > 0 || status !== "all";
  const isEmpty = !isLoading && processPage.items.length === 0;

  return (
    <ScreenScaffold
      className="process-history-screen"
      title={t("process_history.title", "Process History")}
      actions={
        <Button
          disabled={isLoading || isRefreshing}
          leadingIcon="refresh"
          onClick={() => {
            void loadHistory(page, "refresh");
          }}
          size="sm"
          variant="secondary"
        >
          {isRefreshing
            ? t("process_history.actions.refreshing", "Refreshing...")
            : t("process_history.actions.refresh", "Refresh")}
        </Button>
      }
    >
      <div className="process-history-toolbar">
        <TextField
          autoComplete="off"
          className="process-history-toolbar__query"
          label={t("process_history.filters.query.label", "Filter archive")}
          leadingIcon="search"
          onChange={(event) => {
            const nextQuery = event.currentTarget.value;
            startTransition(() => {
              setQuery(nextQuery);
              setPage(1);
            });
          }}
          placeholder={t(
            "process_history.filters.query.placeholder",
            "#101, Revolutions, v1.2.0, deadbeef",
          )}
          value={query}
        />
        <SelectField
          className="process-history-toolbar__status"
          label={t("process_history.filters.status.label", "Status")}
          onChange={(event) => {
            const nextStatus = event.currentTarget
              .value as ProcessFeedStatusFilter;
            startTransition(() => {
              setStatus(nextStatus);
              setPage(1);
            });
          }}
          options={statusOptions}
          value={status}
        />
      </div>

      {error ? <p className="feed-banner feed-banner--error">{error}</p> : null}

      {isLoading && processPage.items.length === 0 ? (
        <div className="feed-state">
          <p className="feed-state__title">
            {t("process_history.loading.title", "Loading process history...")}
          </p>
        </div>
      ) : null}

      {isEmpty && !hasFilters ? (
        <div className="feed-state">
          <p className="feed-state__title">
            {t(
              "process_history.empty.none.title",
              "No processes recorded yet.",
            )}
          </p>
        </div>
      ) : null}

      {isEmpty && hasFilters ? (
        <div className="feed-state">
          <p className="feed-state__title">
            {t(
              "process_history.empty.filtered.title",
              "No processes match this filter.",
            )}
          </p>
        </div>
      ) : null}

      {processPage.items.length > 0 ? (
        <div className="process-list process-history-shell" aria-live="polite">
          {processPage.items.map((process) => (
            <ProcessFeedItem
              isCanceling={pendingCancelReleaseRunId === process.release_run_id}
              key={process.release_run_id}
              onOpenDetail={onOpenDetail}
              onRequestCancel={
                onRequestCancel
                  ? (value) => {
                      void handleCancelProcess(value);
                    }
                  : undefined
              }
              process={process}
            />
          ))}
        </div>
      ) : null}

      {processPage.total_pages > 1 ? (
        <footer className="pagination-bar">
          <p className="pagination-bar__summary">
            {t(
              "process_history.pagination.summary",
              "Page {{page}} of {{totalPages}}",
              {
                page: processPage.page,
                totalPages: processPage.total_pages,
              },
            )}
          </p>
          <div className="pagination-bar__actions">
            <Button
              disabled={!processPage.has_previous_page || isLoading}
              onClick={() => {
                startTransition(() => {
                  setPage(processPage.page - 1);
                });
              }}
              size="sm"
              variant="ghost"
            >
              {t("process_history.pagination.previous", "Previous")}
            </Button>
            <Button
              disabled={!processPage.has_next_page || isLoading}
              onClick={() => {
                startTransition(() => {
                  setPage(processPage.page + 1);
                });
              }}
              size="sm"
              variant="secondary"
            >
              {t("process_history.pagination.next", "Next")}
            </Button>
          </div>
        </footer>
      ) : null}
    </ScreenScaffold>
  );
}

function buildProcessHistoryErrorMessage(
  translate: ReturnType<typeof useLocalization>["t"],
  error: unknown,
) {
  if (error instanceof Error && error.message.trim()) {
    return error.message.trim();
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return translate(
    "process_history.error.generic",
    "The process history could not be loaded.",
  );
}
