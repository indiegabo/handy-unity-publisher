import React from "react";

import { Button } from "../Button";
import { Icon } from "../Icon";
import { Badge, MetaItem, MetaRow } from "../Surface";
import type { RepositoryInspectionEntry } from "../../services/projects";

export type ProjectCardProps = {
  repository: RepositoryInspectionEntry;
  highlighted?: boolean;
  onOpen: (repositoryId: number, repositoryName: string) => void;
  onQuickView: (repositoryId: number) => void;
};

const ProjectCard = React.forwardRef<HTMLButtonElement, ProjectCardProps>(
  ({ repository, highlighted, onOpen, onQuickView }, ref) => {
    const showStatusBadge = highlighted || !repository.enabled;

    return (
      <article
        className={joinClassNames(
          "project-list-card",
          highlighted && "project-list-card--highlighted",
        )}
      >
        <button
          ref={ref}
          className="project-list-card__open"
          onClick={() =>
            onOpen(repository.repository_id, repository.repository_name)
          }
          type="button"
        >
          <div className="project-list-card__header">
            <div className="project-list-card__title-block">
              <div className="project-list-card__title-row">
                <h3 className="project-list-card__title">
                  {repository.repository_name}
                </h3>
                {showStatusBadge ? (
                  <div className="project-list-card__badges">
                    {highlighted ? <Badge tone="strong">new</Badge> : null}
                    {!repository.enabled ? (
                      <Badge tone="muted">disabled</Badge>
                    ) : null}
                  </div>
                ) : null}
              </div>

              <p className="project-list-card__copy">{repository.repo_url}</p>
            </div>

            <span className="project-list-card__direction">
              <span className="project-list-card__direction-label">Edit</span>
              <Icon name="arrowUpRight" size={14} />
            </span>
          </div>

          <MetaRow className="project-list-card__meta">
            <MetaItem label="Engine">{repository.engine_kind}</MetaItem>
            <MetaItem label="Poll">
              {`${repository.polling_interval_seconds}s cadence`}
            </MetaItem>
            <MetaItem label="Targets">
              {formatTargetCount(repository.enabled_build_target_count)}
            </MetaItem>
          </MetaRow>

          <p className="project-list-card__summary">
            {buildRepositorySummary(repository)}
          </p>
        </button>

        <div className="project-list-card__actions">
          <Button
            leadingIcon="search"
            onClick={() => onQuickView(repository.repository_id)}
            size="sm"
            variant="ghost"
          >
            Quick view
          </Button>
        </div>
      </article>
    );
  },
);

export default ProjectCard;

function buildRepositorySummary(repository: RepositoryInspectionEntry) {
  const pipelineState = repository.enabled
    ? "Pipeline enabled."
    : "Pipeline disabled.";
  const lastSeenTag = repository.last_seen_tag
    ? `Last seen tag ${repository.last_seen_tag}.`
    : "No baseline tag recorded yet.";
  const publishDestinationCount = repository.publish_targets.length;

  return `${pipelineState} ${lastSeenTag} ${publishDestinationCount} publish destination${publishDestinationCount === 1 ? "" : "s"} registered.`;
}

function formatTargetCount(targetCount: number) {
  return `${targetCount} active target${targetCount === 1 ? "" : "s"}`;
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
