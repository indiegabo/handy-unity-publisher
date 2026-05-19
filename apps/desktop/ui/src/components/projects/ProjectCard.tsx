import React from "react";

import { Icon } from "../Icon";
import { Badge, MetaItem, MetaRow } from "../Surface";
import type { RepositoryInspectionEntry } from "../../services/projects";

export type ProjectCardProps = {
  repository: RepositoryInspectionEntry;
  highlighted?: boolean;
  onOpen: (repositoryId: number, repositoryName: string) => void;
};

const ProjectCard = React.forwardRef<HTMLButtonElement, ProjectCardProps>(
  ({ repository, highlighted, onOpen }, ref) => {
    return (
      <button
        ref={ref}
        type="button"
        className={joinClassNames(
          "project-list-card",
          highlighted && "project-list-card--highlighted",
        )}
        onClick={() =>
          onOpen(repository.repository_id, repository.repository_name)
        }
      >
        <div className="project-list-card__header">
          <div className="project-list-card__title-block">
            <div className="project-list-card__title-row">
              <h3 className="project-list-card__title">
                {repository.repository_name}
              </h3>
              <div className="project-list-card__badges">
                {highlighted ? <Badge tone="strong">new</Badge> : null}
                <Badge tone={repository.enabled ? "strong" : "muted"}>
                  {repository.enabled ? "enabled" : "disabled"}
                </Badge>
              </div>
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
    );
  },
);

export default ProjectCard;

function buildRepositorySummary(repository: RepositoryInspectionEntry) {
  const lastSeenTag = repository.last_seen_tag
    ? `Last seen tag ${repository.last_seen_tag}.`
    : "No baseline tag recorded yet.";
  const publishDestinationCount = repository.publish_targets.length;

  return `${lastSeenTag} ${publishDestinationCount} publish destination${publishDestinationCount === 1 ? "" : "s"} registered.`;
}

function formatTargetCount(targetCount: number) {
  return `${targetCount} active target${targetCount === 1 ? "" : "s"}`;
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
