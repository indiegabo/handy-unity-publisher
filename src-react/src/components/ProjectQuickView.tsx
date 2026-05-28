import { Button } from "./Button";
import FullScreenModal from "./FullScreenModal";
import {
  Badge,
  MetaItem,
  MetaRow,
  SummaryStrip,
  SurfacePanel,
} from "./Surface";
import type { RepositoryInspectionEntry } from "../services/projects";
import {
  buildLocalizedProjectSourceDisplay,
  isLocalWorkspaceSource,
  resolveLocalizedProjectAutomationCadenceLabel,
  resolveLocalizedProjectSourceFieldLabel,
  resolveLocalizedProjectSourceModeSummary,
  resolveProjectSourceValue,
} from "../projectSourcePresentation";
import { useLocalization } from "../LocalizationProvider";

export type ProjectQuickViewResult = "open-project";

type ProjectQuickViewProps = {
  onResolve?: (value?: ProjectQuickViewResult | null) => void;
  repository: RepositoryInspectionEntry;
};

export function ProjectQuickView({
  onResolve,
  repository,
}: ProjectQuickViewProps) {
  const { t } = useLocalization();

  return (
    <FullScreenModal
      className="project-quick-view__modal"
      description={t(
        "projects.quick_view.description",
        "Inspect the project source, automation footprint, and integration posture without leaving the project list.",
      )}
      onResolve={onResolve}
      title={repository.repository_name}
    >
      <div className="project-quick-view">
        <SummaryStrip className="project-quick-view__summary-strip">
          <p className="project-quick-view__repo-url">
            {buildLocalizedProjectSourceDisplay(t, repository)}
          </p>

          <div className="project-quick-view__badges">
            <Badge tone={repository.enabled ? "strong" : "muted"}>
              {repository.enabled
                ? t("projects.quick_view.status.enabled", "enabled")
                : t("projects.quick_view.status.disabled", "disabled")}
            </Badge>
            <Badge tone="muted">{repository.engine_kind}</Badge>
            <Badge tone="muted">
              {formatProjectVisibilityStatus(t, repository.visibility_status)}
            </Badge>
          </div>

          <MetaRow>
            <MetaItem
              label={t("projects.quick_view.summary.targets", "Targets")}
            >
              {formatTargetCount(t, repository.enabled_build_target_count)}
            </MetaItem>
            <MetaItem
              label={t("projects.quick_view.summary.publishes", "Publishes")}
            >
              {formatPublishTargetCount(t, repository.publish_targets.length)}
            </MetaItem>
            <MetaItem label={t("projects.quick_view.summary.mode", "Mode")}>
              {resolveLocalizedProjectSourceModeSummary(t, repository)}
            </MetaItem>
            <MetaItem label={t("projects.quick_view.summary.sync", "Sync")}>
              {resolveLocalizedProjectAutomationCadenceLabel(t, repository)}
            </MetaItem>
          </MetaRow>
        </SummaryStrip>

        <SurfacePanel
          description={t(
            "projects.quick_view.snapshot.description",
            "Current project automation and source posture exposed by the local runtime inspection.",
          )}
          title={t("projects.quick_view.snapshot.title", "Automation Snapshot")}
          tone="inset"
        >
          <MetaRow>
            <MetaItem
              label={resolveLocalizedProjectSourceFieldLabel(t, repository)}
            >
              {resolveProjectSourceValue(repository) ||
                t("projects.quick_view.not_recorded", "not recorded")}
            </MetaItem>
            <MetaItem
              label={t(
                "projects.quick_view.snapshot.last_seen_tag",
                "Last seen tag",
              )}
            >
              {repository.last_seen_tag ||
                t("projects.quick_view.not_recorded", "not recorded")}
            </MetaItem>
            {isLocalWorkspaceSource(repository) ? (
              <MetaItem
                label={t(
                  "projects.quick_view.snapshot.source_mode",
                  "Source mode",
                )}
              >
                {resolveLocalizedProjectSourceModeSummary(t, repository)}
              </MetaItem>
            ) : (
              <MetaItem
                label={t(
                  "projects.quick_view.snapshot.auth_status",
                  "Auth status",
                )}
              >
                {repository.auth_status_message ||
                  t("projects.quick_view.not_recorded", "not recorded")}
              </MetaItem>
            )}
          </MetaRow>
        </SurfacePanel>

        <SurfacePanel
          description={t(
            "projects.quick_view.targets.description",
            "Build targets currently attached to this project pipeline.",
          )}
          title={t("projects.quick_view.targets.title", "Build Targets")}
          tone="inset"
        >
          {repository.build_targets.length === 0 ? (
            <p className="project-quick-view__empty">
              {t(
                "projects.quick_view.targets.empty",
                "No build targets are currently configured for this project.",
              )}
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
            {t("projects.quick_view.actions.open_project", "Open Project")}
          </Button>
        </div>
      </div>
    </FullScreenModal>
  );
}

function formatProjectVisibilityStatus(
  t: ReturnType<typeof useLocalization>["t"],
  visibilityStatus: string | null | undefined,
) {
  const normalizedVisibilityStatus = visibilityStatus?.trim().toLowerCase();

  switch (normalizedVisibilityStatus) {
    case "private":
      return t("projects.quick_view.visibility.private", "private");
    case "public":
      return t("projects.quick_view.visibility.public", "public");
    case "internal":
      return t("projects.quick_view.visibility.internal", "internal");
    case "unknown":
    case "":
    case undefined:
    case null:
      return t("projects.quick_view.visibility.unknown", "visibility unknown");
    default:
      return normalizedVisibilityStatus.replace(/_/g, " ");
  }
}

function formatPublishTargetCount(
  t: ReturnType<typeof useLocalization>["t"],
  targetCount: number,
) {
  return targetCount === 1
    ? t("projects.quick_view.count.publish.one", "1 destination")
    : t("projects.quick_view.count.publish.other", "{{count}} destinations", {
        count: targetCount,
      });
}

function formatTargetCount(
  t: ReturnType<typeof useLocalization>["t"],
  targetCount: number,
) {
  return targetCount === 1
    ? t("projects.quick_view.count.targets.one", "1 active target")
    : t("projects.quick_view.count.targets.other", "{{count}} active targets", {
        count: targetCount,
      });
}
