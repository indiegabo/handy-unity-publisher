import { Button } from "./Button";
import FullScreenModal from "./FullScreenModal";
import type { ProjectWorkerEntry } from "./ProjectWorkersFocusScreen";
import type { RuntimeHealthStatus } from "../services/runtime";

export type WorkerStatusQuickViewResult = "open-project-workers";

type WorkerStatusQuickViewProps = {
  inspectionAvailable: boolean;
  onResolve?: (value?: WorkerStatusQuickViewResult | null) => void;
  projectWorkers: ProjectWorkerEntry[];
  runtimeStatus: RuntimeHealthStatus | null;
};

export function WorkerStatusQuickView({
  inspectionAvailable,
  onResolve,
  projectWorkers,
  runtimeStatus,
}: WorkerStatusQuickViewProps) {
  return (
    <FullScreenModal
      className="worker-status-quick-view__modal"
      description="Inspect the current worker footprint without leaving the main feed, or open the full worker control screen."
      onResolve={onResolve}
      title="Project Workers"
    >
      <div className="worker-status-quick-view">
        <div className="worker-status-quick-view__summary">
          <p className="worker-status-tooltip__project-meta">
            Runtime: {formatRuntimeStatus(runtimeStatus)}
          </p>
          <p className="worker-status-tooltip__project-meta">
            Active projects: {projectWorkers.length}
          </p>
          <p className="worker-status-tooltip__project-meta">
            Enabled targets: {countBuildTargets(projectWorkers)}
          </p>
        </div>

        <div className="worker-status-tooltip__content">
          {!inspectionAvailable ? (
            <p
              className="worker-status-tooltip__empty"
              data-overlay-autofocus
              tabIndex={-1}
            >
              Loading project worker inventory...
            </p>
          ) : null}

          {inspectionAvailable && projectWorkers.length === 0 ? (
            <p
              className="worker-status-tooltip__empty"
              data-overlay-autofocus
              tabIndex={-1}
            >
              No active project workers configured.
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
                        {projectWorker.buildTargets.length} target
                        {projectWorker.buildTargets.length === 1 ? "" : "s"}
                      </p>
                    </div>

                    <p className="worker-status-tooltip__project-meta">
                      Polling every {projectWorker.pollingIntervalSeconds}s.
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
                        {attentionTargetCount} target
                        {attentionTargetCount === 1 ? " needs" : "s need"}{" "}
                        attention.
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
            Open Project Workers
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

function formatRuntimeStatus(status: RuntimeHealthStatus | null) {
  if (!status) {
    return "unavailable";
  }

  return status.replace(/_/g, " ");
}

function joinClassNames(...tokens: Array<string | false>) {
  return tokens.filter(Boolean).join(" ");
}
