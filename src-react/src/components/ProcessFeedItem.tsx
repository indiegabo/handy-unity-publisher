import { useEffect, useState } from "react";
import { Badge, SummaryStrip } from "./Surface";
import type { IconName } from "./Icon";
import { Button, IconButton } from "./Button";
import { VerticalAccordion } from "./VerticalAccordion";
import { useLocalization } from "../LocalizationProvider";
import { subscribeToProcessElapsedClock } from "../services/runtimeEvents";
import {
  formatLocalizedProcessFeedBuildCount,
  formatLocalizedProcessFeedEngineKindBadge,
  formatLocalizedProcessFeedEngineVersionBadge,
  normalizeProcessFeedDisplayStatus,
  resolveProcessFeedElapsedClock,
  resolveProcessFeedStepDetail,
  resolveLocalizedProcessFeedStepLabel,
  type ProcessFeedRecord,
} from "./processFeedPresentation";

type ProcessFeedItemProps = {
  process: ProcessFeedRecord;
  onOpenDetail: (process: ProcessFeedRecord) => void;
  onRequestCancel?: (process: ProcessFeedRecord) => void | Promise<void>;
  isCanceling?: boolean;
};

export function ProcessFeedItem({
  process,
  onOpenDetail,
  onRequestCancel,
  isCanceling = false,
}: ProcessFeedItemProps) {
  const { t } = useLocalization();
  const normalizedStatus = normalizeProcessFeedDisplayStatus(
    process.display_status,
  );
  const isOnHold = normalizedStatus === "on_hold";
  const canRequestCancel = isActiveProcessStatus(normalizedStatus);
  const currentStep = resolveLocalizedProcessFeedStepLabel(
    t,
    process,
    normalizedStatus,
  );
  const onHoldReason = isOnHold ? resolveOnHoldReasonLabel(t, process) : null;
  const stepDetail = isOnHold ? null : resolveProcessFeedStepDetail(process);
  const [elapsedClock, setElapsedClock] = useState(() =>
    resolveProcessFeedElapsedClock(process),
  );

  useEffect(() => {
    setElapsedClock(resolveProcessFeedElapsedClock(process));
  }, [
    process.created_at,
    process.display_status,
    process.finished_at,
    process.release_run_id,
    process.started_at,
  ]);

  useEffect(() => {
    if (!canRequestCancel) {
      return;
    }

    let disposed = false;
    let unsubscribe: (() => void) | undefined;

    void subscribeToProcessElapsedClock(
      process.release_run_id,
      (nextElapsedClock) => {
        if (disposed) {
          return;
        }

        setElapsedClock(nextElapsedClock);
      },
    )
      .then((dispose) => {
        if (disposed) {
          dispose();
          return;
        }

        unsubscribe = dispose;
      })
      .catch(() => {});

    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [canRequestCancel, process.release_run_id]);

  return (
    <article>
      <VerticalAccordion
        animatedBorder={normalizedStatus === "running"}
        bodyClassName="process-item__accordion-body"
        className={joinClassNames(
          "process-item",
          `process-item--${normalizedStatus}`,
        )}
        collapsedToggleLabel={t(
          "process_feed.item.accordion.expand",
          "Expand process #{{releaseRunId}}",
          { releaseRunId: process.release_run_id },
        )}
        expandedToggleLabel={t(
          "process_feed.item.accordion.collapse",
          "Collapse process #{{releaseRunId}}",
          { releaseRunId: process.release_run_id },
        )}
        header={
          <div className="process-item__summary">
            <div className="process-item__summary-main">
              <div className="process-item__title-row">
                <h3 className="process-item__title">
                  <span className="process-item__index">
                    #{process.release_run_id}
                  </span>
                  <span className="process-item__project-name">
                    {process.repository_name}
                  </span>
                </h3>

                {elapsedClock ? (
                  <p className="process-item__elapsed">{elapsedClock}</p>
                ) : null}
              </div>

              <div className="process-item__summary-actions">
                {canRequestCancel && onRequestCancel ? (
                  <Button
                    className="process-item__cancel-button"
                    disabled={isCanceling}
                    leadingIcon="close"
                    onClick={() => {
                      void onRequestCancel(process);
                    }}
                    size="sm"
                    variant="secondary"
                  >
                    {isCanceling
                      ? t(
                          "process_feed.item.actions.canceling",
                          "Interrupting...",
                        )
                      : t(
                          "process_feed.item.actions.cancel",
                          "Interrupt process",
                        )}
                  </Button>
                ) : null}

                <IconButton
                  className={joinClassNames(
                    "process-status-trigger",
                    `process-status-trigger--${normalizedStatus}`,
                  )}
                  icon={resolveStatusIcon(normalizedStatus)}
                  label={t(
                    "process_feed.item.actions.open_detail",
                    "Open process detail #{{releaseRunId}}",
                    { releaseRunId: process.release_run_id },
                  )}
                  onClick={() => onOpenDetail(process)}
                  size="sm"
                  variant="ghost"
                />
              </div>
            </div>

            <SummaryStrip className="process-item__summary-strip">
              <div className="process-item__badges">
                <Badge className="process-item__badge" tone="neutral">
                  {process.git_tag}
                </Badge>
                <Badge className="process-item__badge" tone="muted">
                  {formatLocalizedProcessFeedEngineKindBadge(
                    t,
                    process.repository_engine_kind,
                  )}
                </Badge>
                <Badge className="process-item__badge" tone="muted">
                  {formatLocalizedProcessFeedEngineVersionBadge(
                    t,
                    process.engine_version,
                  )}
                </Badge>
                <Badge className="process-item__badge" tone="muted">
                  {formatLocalizedProcessFeedBuildCount(
                    t,
                    process.total_build_runs,
                  )}
                </Badge>
              </div>

              {onHoldReason ? (
                <p className="process-item__on-hold-reason">{onHoldReason}</p>
              ) : null}
            </SummaryStrip>
          </div>
        }
        triggerMode="button"
      >
        <div className="process-item__body">
          <div className="process-item__step-row">
            <p className="process-item__step">{currentStep}</p>
          </div>

          {stepDetail ? (
            <p className="process-item__step-detail">{stepDetail}</p>
          ) : null}
        </div>
      </VerticalAccordion>
    </article>
  );
}

function resolveStatusIcon(status: string): IconName {
  switch (status) {
    case "queued":
      return "box";
    case "running":
      return "play";
    case "on_hold":
      return "alertCircle";
    case "succeeded":
      return "checkCircle";
    case "failed":
    case "canceled":
      return "alertCircle";
    default:
      return "box";
  }
}

function resolveOnHoldReasonLabel(
  translate: ReturnType<typeof useLocalization>["t"],
  process: ProcessFeedRecord,
) {
  const detail = process.current_step_detail?.trim();
  if (detail) {
    return detail;
  }

  return translate(
    "process_feed.on_hold.reason",
    "On hold because Unity Editor is open for this local workspace. Close Unity to resume, or interrupt this process.",
  );
}

function isActiveProcessStatus(status: string) {
  const normalizedStatus = normalizeProcessFeedDisplayStatus(status);
  return (
    normalizedStatus !== "succeeded" &&
    normalizedStatus !== "failed" &&
    normalizedStatus !== "canceled"
  );
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
