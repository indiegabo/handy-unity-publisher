import { Badge, SummaryStrip } from "./Surface";
import type { IconName } from "./Icon";
import { IconButton } from "./Button";
import { VerticalAccordion } from "./VerticalAccordion";
import { useLocalization } from "../LocalizationProvider";
import {
  formatLocalizedProcessFeedBuildCount,
  formatLocalizedProcessFeedEngineKindBadge,
  formatLocalizedProcessFeedEngineVersionBadge,
  normalizeProcessFeedDisplayStatus,
  resolveLocalizedProcessFeedStepLabel,
  type ProcessFeedRecord,
} from "./processFeedPresentation";

type ProcessFeedItemProps = {
  process: ProcessFeedRecord;
  onOpenDetail: (process: ProcessFeedRecord) => void;
};

export function ProcessFeedItem({
  process,
  onOpenDetail,
}: ProcessFeedItemProps) {
  const { t } = useLocalization();
  const normalizedStatus = normalizeProcessFeedDisplayStatus(
    process.display_status,
  );
  const currentStep = resolveLocalizedProcessFeedStepLabel(
    t,
    process,
    normalizedStatus,
  );

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
              <h3 className="process-item__title">
                <span className="process-item__index">
                  #{process.release_run_id}
                </span>
                <span className="process-item__project-name">
                  {process.repository_name}
                </span>
              </h3>

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
            </SummaryStrip>
          </div>
        }
        triggerMode="button"
      >
        <div className="process-item__body">
          <p className="process-item__step">{currentStep}</p>
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
    case "succeeded":
      return "checkCircle";
    case "failed":
    case "canceled":
      return "alertCircle";
    default:
      return "box";
  }
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
