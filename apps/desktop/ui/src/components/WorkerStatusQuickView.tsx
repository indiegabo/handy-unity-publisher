import { useLocalization } from "../LocalizationProvider";
import { Button } from "./Button";
import FullScreenModal from "./FullScreenModal";
import { MetaItem, MetaRow, SummaryStrip } from "./Surface";
import type { ProjectWorkerEntry } from "./ProjectWorkersFocusScreen";
import type {
  RuntimeAutomationMode,
  RuntimeHealthStatus,
} from "../services/runtime";

export type WorkerStatusQuickViewResult = "open-project-workers";

type WorkerStatusQuickViewProps = {
  automationMode: RuntimeAutomationMode | null;
  inspectionAvailable: boolean;
  onResolve?: (value?: WorkerStatusQuickViewResult | null) => void;
  projectWorkers: ProjectWorkerEntry[];
  runtimeStatus: RuntimeHealthStatus | null;
};

export function WorkerStatusQuickView({
  automationMode,
  inspectionAvailable,
  onResolve,
  projectWorkers,
  runtimeStatus,
}: WorkerStatusQuickViewProps) {
  const { t } = useLocalization();

  return (
    <FullScreenModal
      className="worker-status-quick-view__modal"
      description={t(
        "app.worker_quick_view.description",
        "Inspect the current worker footprint without leaving the main feed, or open the full worker control screen.",
      )}
      onResolve={onResolve}
      title={t("app.worker_quick_view.title", "Project Workers")}
    >
      <div className="worker-status-quick-view">
        <SummaryStrip className="worker-status-quick-view__summary-strip">
          <MetaRow className="worker-status-quick-view__summary-row">
            <MetaItem
              label={t("app.worker_quick_view.summary.runtime", "Runtime")}
            >
              {formatRuntimeStatus(t, runtimeStatus)}
            </MetaItem>
            <MetaItem
              label={t(
                "app.worker_quick_view.summary.automation",
                "Automation",
              )}
            >
              {formatAutomationMode(t, automationMode)}
            </MetaItem>
            <MetaItem
              label={t(
                "app.worker_quick_view.summary.active_projects",
                "Active projects",
              )}
            >
              {projectWorkers.length}
            </MetaItem>
            <MetaItem
              label={t(
                "app.worker_quick_view.summary.enabled_targets",
                "Enabled targets",
              )}
            >
              {countBuildTargets(projectWorkers)}
            </MetaItem>
          </MetaRow>
        </SummaryStrip>

        {automationMode === "idle" ? (
          <p className="worker-status-quick-view__notice">
            {t(
              "app.worker_quick_view.notice.polling_paused",
              "Automatic polling is paused. Manual instant checks remain available from Project Workers.",
            )}
          </p>
        ) : null}

        <div className="worker-status-tooltip__content">
          {!inspectionAvailable ? (
            <p
              className="worker-status-tooltip__empty"
              data-overlay-autofocus
              tabIndex={-1}
            >
              {t(
                "app.worker_quick_view.loading",
                "Loading project worker inventory...",
              )}
            </p>
          ) : null}

          {inspectionAvailable && projectWorkers.length === 0 ? (
            <p
              className="worker-status-tooltip__empty"
              data-overlay-autofocus
              tabIndex={-1}
            >
              {t(
                "app.worker_quick_view.empty",
                "No active project workers configured.",
              )}
            </p>
          ) : null}

          {inspectionAvailable && projectWorkers.length > 0 ? (
            <ul className="worker-status-tooltip__list">
              {projectWorkers.map((projectWorker) => {
                const attentionTargetCount = projectWorker.buildTargets.filter(
                  (buildTarget) => buildTarget.diagnosticStatus !== "ready",
                ).length;

                return (
                  <li
                    className="worker-status-tooltip__item"
                    key={projectWorker.repositoryId}
                  >
                    <div className="worker-status-tooltip__project-row">
                      <p className="worker-status-tooltip__project-name">
                        {projectWorker.repositoryName}
                      </p>
                      <p className="worker-status-tooltip__project-meta">
                        {formatTargetCount(
                          t,
                          projectWorker.buildTargets.length,
                        )}
                      </p>
                    </div>

                    <p className="worker-status-tooltip__project-meta">
                      {t(
                        "app.worker_quick_view.polling_interval",
                        "Polling every {{seconds}}s.",
                        { seconds: projectWorker.pollingIntervalSeconds },
                      )}
                    </p>

                    <div className="worker-status-tooltip__worker-list">
                      {projectWorker.buildTargets.map((buildTarget) => (
                        <span
                          className={joinClassNames(
                            "worker-status-tooltip__worker",
                            buildTarget.diagnosticStatus !== "ready" &&
                              "worker-status-tooltip__worker--warning",
                          )}
                          key={buildTarget.buildTargetId}
                        >
                          {buildTarget.name}
                        </span>
                      ))}
                    </div>

                    {attentionTargetCount > 0 ? (
                      <p className="worker-status-tooltip__project-meta">
                        {t(
                          "app.worker_quick_view.attention_required",
                          "{{targetCount}} requiring attention.",
                          {
                            targetCount: formatTargetCount(
                              t,
                              attentionTargetCount,
                            ),
                          },
                        )}
                      </p>
                    ) : null}
                  </li>
                );
              })}
            </ul>
          ) : null}
        </div>

        <div className="worker-status-quick-view__actions">
          <Button
            data-overlay-autofocus={
              inspectionAvailable && projectWorkers.length > 0
            }
            leadingIcon="layout"
            onClick={() => onResolve?.("open-project-workers")}
            size="sm"
            variant="primary"
          >
            {t(
              "app.worker_quick_view.open_project_workers",
              "Open Project Workers",
            )}
          </Button>
        </div>
      </div>
    </FullScreenModal>
  );
}

function countBuildTargets(projectWorkers: ProjectWorkerEntry[]) {
  return projectWorkers.reduce(
    (total, projectWorker) => total + projectWorker.buildTargets.length,
    0,
  );
}

function formatRuntimeStatus(
  t: ReturnType<typeof useLocalization>["t"],
  status: RuntimeHealthStatus | null,
) {
  if (!status) {
    return t("app.runtime_status.unavailable", "unavailable");
  }

  switch (status) {
    case "bootstrapping":
      return t("app.runtime_status.bootstrapping", "bootstrapping");
    case "healthy":
      return t("app.runtime_status.healthy", "healthy");
    case "shutting_down":
      return t("app.runtime_status.shutting_down", "shutting down");
    case "stopped":
      return t("app.runtime_status.stopped", "stopped");
    case "unhealthy":
      return t("app.runtime_status.unhealthy", "unhealthy");
  }
}

function formatAutomationMode(
  t: ReturnType<typeof useLocalization>["t"],
  mode: RuntimeAutomationMode | null,
) {
  if (!mode) {
    return t("app.runtime_status.unavailable", "unavailable");
  }

  return mode === "idle"
    ? t("app.worker_quick_view.automation.paused", "paused")
    : t("app.worker_quick_view.automation.active", "active");
}

function formatTargetCount(
  t: ReturnType<typeof useLocalization>["t"],
  targetCount: number,
) {
  return targetCount === 1
    ? t("app.worker_quick_view.count.target.one", "1 target")
    : t("app.worker_quick_view.count.target.other", "{{count}} targets", {
        count: targetCount,
      });
}

function joinClassNames(...tokens: Array<string | false>) {
  return tokens.filter(Boolean).join(" ");
}
