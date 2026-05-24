import React from "react";

import { Button } from "../Button";
import { Icon } from "../Icon";
import { Badge, MetaItem, MetaRow, SummaryStrip } from "../Surface";
import type { RepositoryInspectionEntry } from "../../services/projects";
import {
  buildLocalizedProjectSourceDisplay,
  resolveLocalizedProjectAutomationCadenceLabel,
  resolveLocalizedProjectSourceModeSummary,
} from "../../projectSourcePresentation";
import { useLocalization } from "../../LocalizationProvider";

export type ProjectCardProps = {
  repository: RepositoryInspectionEntry;
  highlighted?: boolean;
  onCardKeyDown?: (
    repositoryId: number,
    event: React.KeyboardEvent<HTMLButtonElement>,
  ) => void;
  onOpen: (repositoryId: number, repositoryName: string) => void;
  onQuickView: (repositoryId: number) => void;
};

const ProjectCard = React.forwardRef<HTMLButtonElement, ProjectCardProps>(
  ({ repository, highlighted, onCardKeyDown, onOpen, onQuickView }, ref) => {
    const { t } = useLocalization();
    const showStatusBadge = highlighted || !repository.enabled;

    return (
      <article
        className={joinClassNames(
          "project-list-card",
          highlighted && "project-list-card--highlighted",
        )}
      >
        <button
          aria-label={t(
            "projects.card.actions.open_project_named",
            "Open project {{repositoryName}}",
            {
              repositoryName: repository.repository_name,
            },
          )}
          ref={ref}
          className="project-list-card__open"
          onKeyDown={(event) =>
            onCardKeyDown?.(repository.repository_id, event)
          }
          onClick={() =>
            onOpen(repository.repository_id, repository.repository_name)
          }
          title={t(
            "projects.card.actions.open_project_named",
            "Open project {{repositoryName}}",
            {
              repositoryName: repository.repository_name,
            },
          )}
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
                    {highlighted ? (
                      <Badge tone="strong">
                        {t("projects.card.badges.new", "new")}
                      </Badge>
                    ) : null}
                    {!repository.enabled ? (
                      <Badge tone="muted">
                        {t("projects.card.badges.disabled", "disabled")}
                      </Badge>
                    ) : null}
                  </div>
                ) : null}
              </div>

              <p className="project-list-card__copy">
                {buildLocalizedProjectSourceDisplay(t, repository)}
              </p>
            </div>

            <span className="project-list-card__direction">
              <span className="project-list-card__direction-label">
                {t("projects.card.direction.edit", "Edit")}
              </span>
              <Icon name="arrowUpRight" size={14} />
            </span>
          </div>

          <SummaryStrip className="project-list-card__summary-strip">
            <MetaRow className="project-list-card__meta">
              <MetaItem label={t("projects.card.summary.engine", "Engine")}>
                {repository.engine_kind}
              </MetaItem>
              <MetaItem label={t("projects.card.summary.mode", "Mode")}>
                {resolveLocalizedProjectSourceModeSummary(t, repository)}
              </MetaItem>
              <MetaItem label={t("projects.card.summary.targets", "Targets")}>
                {formatTargetCount(t, repository.enabled_build_target_count)}
              </MetaItem>
            </MetaRow>

            <p className="project-list-card__summary">
              {buildRepositorySummary(t, repository)}
            </p>
          </SummaryStrip>
        </button>

        <div className="project-list-card__actions">
          <Button
            aria-label={t(
              "projects.card.actions.quick_view_named",
              "Quick view for {{repositoryName}}",
              {
                repositoryName: repository.repository_name,
              },
            )}
            leadingIcon="search"
            onClick={() => onQuickView(repository.repository_id)}
            size="sm"
            title={t(
              "projects.card.actions.quick_view_named",
              "Quick view for {{repositoryName}}",
              {
                repositoryName: repository.repository_name,
              },
            )}
            variant="ghost"
          >
            {t("projects.card.actions.quick_view", "Quick view")}
          </Button>
        </div>
      </article>
    );
  },
);

export default ProjectCard;

function buildRepositorySummary(
  t: ReturnType<typeof useLocalization>["t"],
  repository: RepositoryInspectionEntry,
) {
  const sourceMode = resolveLocalizedProjectAutomationCadenceLabel(
    t,
    repository,
  );
  const pipelineState = repository.enabled
    ? t("projects.card.pipeline.enabled", "Pipeline enabled.")
    : t("projects.card.pipeline.disabled", "Pipeline disabled.");
  const lastSeenTag = repository.last_seen_tag
    ? t("projects.card.last_seen_tag.known", "Last seen tag {{tag}}.", {
        tag: repository.last_seen_tag,
      })
    : t("projects.card.last_seen_tag.none", "No baseline tag recorded yet.");
  const publishDestinationCount = repository.publish_targets.length;

  const publishSummary =
    publishDestinationCount === 1
      ? t(
          "projects.card.publish_destinations.one",
          "1 publish destination registered.",
        )
      : t(
          "projects.card.publish_destinations.other",
          "{{count}} publish destinations registered.",
          { count: publishDestinationCount },
        );

  return `${sourceMode}. ${pipelineState} ${lastSeenTag} ${publishSummary}`;
}

function formatTargetCount(
  t: ReturnType<typeof useLocalization>["t"],
  targetCount: number,
) {
  return targetCount === 1
    ? t("projects.card.count.targets.one", "1 active target")
    : t("projects.card.count.targets.other", "{{count}} active targets", {
        count: targetCount,
      });
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
