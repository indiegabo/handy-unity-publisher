import { Badge, SummaryStrip } from "./Surface";
import type { IconName } from "./Icon";
import { IconButton } from "./Button";
import { VerticalAccordion } from "./VerticalAccordion";
import {
  formatProcessFeedBuildCount,
  formatProcessFeedEngineKindBadge,
  formatProcessFeedEngineVersionBadge,
  normalizeProcessFeedDisplayStatus,
  resolveProcessFeedStepLabel,
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
  const normalizedStatus = normalizeProcessFeedDisplayStatus(
    process.display_status,
  );
  const currentStep = resolveProcessFeedStepLabel(process, normalizedStatus);

  return (
    <article>
      <VerticalAccordion
        animatedBorder={normalizedStatus === "running"}
        bodyClassName="process-item__accordion-body"
        className={joinClassNames(
          "process-item",
          `process-item--${normalizedStatus}`,
        )}
        collapsedToggleLabel={`Expand process #${process.release_run_id}`}
        expandedToggleLabel={`Collapse process #${process.release_run_id}`}
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
                label={`Open process detail #${process.release_run_id}`}
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
                  {formatProcessFeedEngineKindBadge(
                    process.repository_engine_kind,
                  )}
                </Badge>
                <Badge className="process-item__badge" tone="muted">
                  {formatProcessFeedEngineVersionBadge(process.engine_version)}
                </Badge>
                <Badge className="process-item__badge" tone="muted">
                  {formatProcessFeedBuildCount(process.total_build_runs)}
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
