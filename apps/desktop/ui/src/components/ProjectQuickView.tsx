import { Button } from "./Button";
import FullScreenModal from "./FullScreenModal";
import { Badge, MetaItem, MetaRow, SummaryStrip, SurfacePanel } from "./Surface";
import type { RepositoryInspectionEntry } from "../services/projects";

export type ProjectQuickViewResult = "open-project";

type ProjectQuickViewProps = {
  onResolve?: (value?: ProjectQuickViewResult | null) => void;
  repository: RepositoryInspectionEntry;
};

export function ProjectQuickView({
  onResolve,
  repository,
}: ProjectQuickViewProps) {
  return (
    <FullScreenModal
      className="project-quick-view__modal"
      description="Inspect the repository identity, automation footprint, and integration posture without leaving the project list."
      onResolve={onResolve}
      title={repository.repository_name}
    >
      <div className="project-quick-view">
        <SummaryStrip className="project-quick-view__summary-strip">
          <p className="project-quick-view__repo-url">{repository.repo_url}</p>

          <div className="project-quick-view__badges">
            <Badge tone={repository.enabled ? "strong" : "muted"}>
              {repository.enabled ? "enabled" : "disabled"}
            </Badge>
            <Badge tone="muted">{repository.engine_kind}</Badge>
            <Badge tone="muted">
              {repository.visibility_status || "visibility unknown"}
            </Badge>
          </div>

          <MetaRow>
            <MetaItem label="Targets">
              {formatTargetCount(repository.enabled_build_target_count)}
            </MetaItem>
            <MetaItem label="Publishes">
              {formatPublishTargetCount(repository.publish_targets.length)}
            </MetaItem>
            <MetaItem label="Polling">
              {`${repository.polling_interval_seconds}s cadence`}
            </MetaItem>
          </MetaRow>
        </SummaryStrip>

        <SurfacePanel
          description="Current repository automation and credential posture exposed by the local runtime inspection."
          title="Automation Snapshot"
          tone="inset"
        >
          <MetaRow>
            <MetaItem label="Default branch">
              {repository.default_branch || "not recorded"}
            </MetaItem>
            <MetaItem label="Last seen tag">
              {repository.last_seen_tag || "not recorded"}
            </MetaItem>
            <MetaItem label="Auth status">
              {repository.auth_status_message || "not recorded"}
            </MetaItem>
          </MetaRow>
        </SurfacePanel>

        <SurfacePanel
          description="Build targets currently attached to this repository pipeline."
          title="Build Targets"
          tone="inset"
        >
          {repository.build_targets.length === 0 ? (
            <p className="project-quick-view__empty">
              No build targets are currently configured for this repository.
            </p>
          ) : (
            <div className="project-quick-view__target-list">
              {repository.build_targets.map((target) => (
                <div
                  className="project-quick-view__target"
                  key={target.build_target_id}
                >
                  <p className="project-quick-view__target-title">
                    {target.target_name}
                  </p>
                  <p className="project-quick-view__target-copy">
                    {target.unity_target_platform}
                  </p>
                </div>
              ))}
            </div>
          )}
        </SurfacePanel>

        <div className="project-quick-view__actions">
          <Button
            data-overlay-autofocus
            leadingIcon="arrowUpRight"
            onClick={() => onResolve?.("open-project")}
            size="sm"
            variant="primary"
          >
            Open Project
          </Button>
        </div>
      </div>
    </FullScreenModal>
  );
}

function formatPublishTargetCount(targetCount: number) {
  return `${targetCount} destination${targetCount === 1 ? "" : "s"}`;
}

function formatTargetCount(targetCount: number) {
  return `${targetCount} active target${targetCount === 1 ? "" : "s"}`;
}
