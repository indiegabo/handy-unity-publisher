import { Badge } from "./Surface";
import { Icon, type IconName } from "./Icon";
import { VerticalAccordion } from "./VerticalAccordion";

export type ProcessFeedRecord = {
  release_run_id: number;
  repository_id: number;
  repository_name: string;
  repository_url: string;
  git_tag: string;
  git_commit: string | null;
  unity_version: string | null;
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
};

export function ProcessFeedItem({ process }: ProcessFeedItemProps) {
  const normalizedStatus = normalizeDisplayStatus(process.display_status);
  const currentStep = resolveCurrentStepLabel(process, normalizedStatus);
  const showStatusIcon = shouldShowStatusIcon(normalizedStatus);

  return (
    <article>
      <VerticalAccordion
        animatedBorder={normalizedStatus === "running"}
        bodyClassName="process-item__accordion-body"
        className={joinClassNames("process-item", `process-item--${normalizedStatus}`)}
        collapsedToggleLabel={`Expand process #${process.release_run_id}`}
        expandedToggleLabel={`Collapse process #${process.release_run_id}`}
        header={
          <div
            className={joinClassNames(
              "process-item__summary",
              !showStatusIcon && "process-item__summary--without-status",
            )}
          >
            <h3 className="process-item__title">
              <span className="process-item__index">#{process.release_run_id}</span>
              <span className="process-item__project-name">{process.repository_name}</span>
            </h3>

            {showStatusIcon ? (
              <div className="process-item__status">
                <span
                  className={joinClassNames(
                    "process-status-icon",
                    `process-status-icon--${normalizedStatus}`,
                  )}
                  aria-label={formatStatusLabel(normalizedStatus)}
                  title={formatStatusLabel(normalizedStatus)}
                >
                  <Icon
                    className="process-status-icon__glyph"
                    name={resolveStatusIcon(normalizedStatus)}
                    size={16}
                  />
                </span>
              </div>
            ) : null}
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
              {formatUnityBadge(process.unity_version)}
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

function formatUnityBadge(unityVersion: string | null) {
  if (unityVersion?.trim()) {
    return `Unity ${unityVersion.trim()}`;
  }

  return "Unity pending";
}

function formatBuildCount(totalBuildRuns: number) {
  if (totalBuildRuns === 1) {
    return "1 build";
  }

  return `${totalBuildRuns} builds`;
}

function resolveStatusIcon(status: string): IconName {
  switch (status) {
    case "succeeded":
      return "checkCircle";
    case "failed":
    case "canceled":
      return "alertCircle";
    default:
      return "refresh";
  }
}

function formatStatusLabel(status: string) {
  switch (status) {
    case "queued":
      return "Queued";
    case "succeeded":
      return "Success";
    case "failed":
    case "canceled":
      return "Error";
    default:
      return "Processing";
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

function shouldShowStatusIcon(status: string) {
  return status === "succeeded" || status === "failed" || status === "canceled";
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}