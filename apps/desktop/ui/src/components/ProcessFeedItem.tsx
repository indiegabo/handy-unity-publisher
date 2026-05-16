import { Badge } from "./Surface";
import type { IconName } from "./Icon";
import { IconButton } from "./Button";
import { VerticalAccordion } from "./VerticalAccordion";

export type ProcessFeedRecord = {
  release_run_id: number;
  repository_id: number;
  repository_name: string;
  repository_url: string;
  repository_engine_kind: string;
  git_tag: string;
  git_commit: string | null;
  engine_version: string | null;
  display_status: string;
  current_step_label: string;
  current_step_status: string;
  current_step_detail: string | null;
  queued_build_runs: number;
  running_build_runs: number;
  succeeded_build_runs: number;
  failed_build_runs: number;
  canceled_build_runs: number;
  queued_publish_runs: number;
  running_publish_runs: number;
  succeeded_publish_runs: number;
  failed_publish_runs: number;
  canceled_publish_runs: number;
  total_build_runs: number;
  total_publish_runs: number;
  started_at: string | null;
  finished_at: string | null;
  error_message: string | null;
  created_at: string;
  updated_at: string;
};

type ProcessFeedItemProps = {
  process: ProcessFeedRecord;
  onOpenDetail: (process: ProcessFeedRecord) => void;
};

export function ProcessFeedItem({
  process,
  onOpenDetail,
}: ProcessFeedItemProps) {
  const normalizedStatus = normalizeDisplayStatus(process.display_status);
  const currentStep = resolveCurrentStepLabel(process, normalizedStatus);

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
                label={`Abrir detalhe do processo #${process.release_run_id}`}
                onClick={() => onOpenDetail(process)}
                size="sm"
                variant="ghost"
              />
            </div>
          </div>
        }
        triggerMode="button"
      >
        <div className="process-item__body">
          <p className="process-item__step">{currentStep}</p>

          <div className="process-item__badges">
            <Badge className="process-item__badge" tone="neutral">
              {process.git_tag}
            </Badge>
            <Badge className="process-item__badge" tone="muted">
              {formatEngineKindBadge(process.repository_engine_kind)}
            </Badge>
            <Badge className="process-item__badge" tone="muted">
              {formatEngineVersionBadge(process.engine_version)}
            </Badge>
            <Badge className="process-item__badge" tone="muted">
              {formatBuildCount(process.total_build_runs)}
            </Badge>
          </div>
        </div>
      </VerticalAccordion>
    </article>
  );
}

function resolveCurrentStepLabel(process: ProcessFeedRecord, status: string) {
  return (
    process.current_step_label.trim() ||
    process.current_step_detail?.trim() ||
    buildFallbackStep(status)
  );
}

function buildFallbackStep(status: string) {
  switch (status) {
    case "queued":
      return "The runtime is still planning this process.";
    case "running":
      return "The runtime is still updating this process.";
    case "succeeded":
      return "All recorded work for this process finished cleanly.";
    case "failed":
      return "At least one build or publish task failed.";
    case "canceled":
      return "The process stopped before every child task finished.";
    default:
      return "The runtime is still planning this process.";
  }
}

function formatEngineVersionBadge(engineVersion: string | null) {
  if (engineVersion?.trim()) {
    return `Engine ${engineVersion.trim()}`;
  }

  return "Engine pending";
}

function formatEngineKindBadge(engineKind: string) {
  const normalized = engineKind.trim();
  if (!normalized) {
    return "engine: unknown";
  }

  return `engine: ${normalized}`;
}

function formatBuildCount(totalBuildRuns: number) {
  if (totalBuildRuns === 1) {
    return "1 build";
  }

  return `${totalBuildRuns} builds`;
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

function normalizeDisplayStatus(status: string) {
  switch (status) {
    case "queued":
    case "running":
    case "succeeded":
    case "failed":
    case "canceled":
      return status;
    default:
      return "queued";
  }
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
