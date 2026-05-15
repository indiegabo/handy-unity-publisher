import { Button } from "./Button";
import { Badge, SurfacePanel } from "./Surface";
import type { RuntimeHealthStatus } from "../services/runtime";

export type RuntimeControlAction = "start" | "stop" | "restart";

export type BuildTargetEntry = {
  buildTargetId: number;
  diagnosticMessage: string;
  diagnosticStatus: string;
  name: string;
  platform: string;
};

export type ProjectWorkerEntry = {
  pollingIntervalSeconds: number;
  repositoryId: number;
  repositoryName: string;
  buildTargets: BuildTargetEntry[];
};

type ProjectWorkersFocusScreenProps = {
  actionError: string | null;
  actionMessage: string | null;
  inspectionAvailable: boolean;
  onInstantCheck: (repositoryId: number, repositoryName: string) => void;
  onRestartRuntime: () => void;
  onStartRuntime: () => void;
  onStopRuntime: () => void;
  pendingInstantCheckRepositoryId: number | null;
  pendingRuntimeAction: RuntimeControlAction | null;
  projectWorkers: ProjectWorkerEntry[];
  runtimeStatus: RuntimeHealthStatus | null;
};

export function ProjectWorkersFocusScreen({
  actionError,
  actionMessage,
  inspectionAvailable,
  onInstantCheck,
  onRestartRuntime,
  onStartRuntime,
  onStopRuntime,
  pendingInstantCheckRepositoryId,
  pendingRuntimeAction,
  projectWorkers,
  runtimeStatus,
}: ProjectWorkersFocusScreenProps) {
  const runtimeIsRunning = runtimeStatus ? runtimeStatus !== "stopped" : false;
  const runtimeBusy = pendingRuntimeAction !== null;

  return (
    <div className="project-workers-focus-shell">
      <SurfacePanel
        actions={
          <div className="project-workers-focus-toolbar">
            <Button
              disabled={runtimeBusy || runtimeIsRunning}
              leadingIcon="play"
              onClick={onStartRuntime}
              size="sm"
              variant="secondary"
            >
              {pendingRuntimeAction === "start" ? "Starting..." : "Start"}
            </Button>
            <Button
              disabled={runtimeBusy || !runtimeIsRunning}
              onClick={onStopRuntime}
              size="sm"
              variant="ghost"
            >
              {pendingRuntimeAction === "stop" ? "Stopping..." : "Stop"}
            </Button>
            <Button
              disabled={runtimeBusy}
              leadingIcon="refresh"
              onClick={onRestartRuntime}
              size="sm"
              variant="secondary"
            >
              {pendingRuntimeAction === "restart" ? "Restarting..." : "Restart"}
            </Button>
          </div>
        }
        description="Control the local automation runtime and trigger immediate repository checks."
        eyebrow="Runtime"
        title="Runtime Controls"
      >
        <div className="project-workers-focus-status-row">
          <Badge tone={resolveRuntimeBadgeTone(runtimeStatus)}>
            {formatRuntimeStatus(runtimeStatus)}
          </Badge>
          <p className="project-workers-focus-copy">
            {buildRuntimeCopy(runtimeStatus)}
          </p>
        </div>
      </SurfacePanel>

      {actionMessage ? <p className="notice-banner">{actionMessage}</p> : null}
      {actionError ? (
        <p className="feed-banner feed-banner--error">{actionError}</p>
      ) : null}

      <SurfacePanel
        description="Repositories that currently expose enabled build targets."
        eyebrow="Inventory"
        title="Project Workers"
      >
        {!inspectionAvailable ? (
          <div className="feed-state">
            <p className="feed-state__title">
              Loading project worker inventory...
            </p>
            <p className="feed-state__copy">
              The shell is refreshing the repositories that currently expose
              enabled build targets.
            </p>
          </div>
        ) : null}

        {inspectionAvailable && projectWorkers.length === 0 ? (
          <div className="feed-state">
            <p className="feed-state__title">
              No active project workers configured.
            </p>
            <p className="feed-state__copy">
              Enabled repositories need at least one enabled build target before
              they appear here.
            </p>
          </div>
        ) : null}

        {inspectionAvailable && projectWorkers.length > 0 ? (
          <div className="project-workers-focus-project-list">
            {projectWorkers.map((projectWorker) => (
              <section
                className="project-workers-focus-card"
                key={projectWorker.repositoryId}
              >
                <div className="project-workers-focus-card__header">
                  <div className="project-workers-focus-card__title-block">
                    <h3 className="project-workers-focus-card__title">
                      {projectWorker.repositoryName}
                    </h3>
                    <p className="project-workers-focus-card__copy">
                      Poll every {projectWorker.pollingIntervalSeconds}s
                    </p>
                  </div>

                  <Button
                    disabled={
                      runtimeBusy || pendingInstantCheckRepositoryId !== null
                    }
                    leadingIcon="refresh"
                    onClick={() =>
                      onInstantCheck(
                        projectWorker.repositoryId,
                        projectWorker.repositoryName,
                      )
                    }
                    size="sm"
                    variant="secondary"
                  >
                    {pendingInstantCheckRepositoryId ===
                    projectWorker.repositoryId
                      ? "Checking..."
                      : "Instant Check"}
                  </Button>
                </div>

                <div className="project-workers-focus-card__meta">
                  <Badge tone="neutral">
                    {projectWorker.buildTargets.length} build target
                    {projectWorker.buildTargets.length === 1 ? "" : "s"}
                  </Badge>
                </div>

                <div className="project-workers-focus-build-target-list">
                  {projectWorker.buildTargets.map((buildTarget) => (
                    <article
                      className="project-workers-focus-build-target-chip"
                      key={buildTarget.buildTargetId}
                    >
                      <div className="project-workers-focus-build-target-chip__header">
                        <div className="project-workers-focus-build-target-chip__title-block">
                          <p className="project-workers-focus-build-target-chip__title">
                            {buildTarget.name}
                          </p>
                          <p className="project-workers-focus-build-target-chip__copy">
                            {buildTarget.platform}
                          </p>
                        </div>

                        <Badge
                          tone={resolveBuildTargetBadgeTone(
                            buildTarget.diagnosticStatus,
                          )}
                        >
                          {buildTarget.diagnosticStatus.replace(/_/g, " ")}
                        </Badge>
                      </div>

                      <p className="project-workers-focus-build-target-chip__copy">
                        {buildTarget.diagnosticMessage}
                      </p>
                    </article>
                  ))}
                </div>
              </section>
            ))}
          </div>
        ) : null}
      </SurfacePanel>
    </div>
  );
}

function resolveRuntimeBadgeTone(runtimeStatus: RuntimeHealthStatus | null) {
  if (runtimeStatus === "healthy") {
    return "strong";
  }

  if (runtimeStatus === "unhealthy") {
    return "neutral";
  }

  return "muted";
}

function resolveBuildTargetBadgeTone(diagnosticStatus: string) {
  return diagnosticStatus === "ready" ? "strong" : "muted";
}

function formatRuntimeStatus(runtimeStatus: RuntimeHealthStatus | null) {
  if (!runtimeStatus) {
    return "health unavailable";
  }

  return runtimeStatus.replace(/_/g, " ");
}

function buildRuntimeCopy(runtimeStatus: RuntimeHealthStatus | null) {
  if (!runtimeStatus) {
    return "The shell is still resolving the latest runtime health snapshot.";
  }

  if (runtimeStatus === "healthy") {
    return "The runtime is serving the local automation host normally.";
  }

  if (runtimeStatus === "unhealthy") {
    return "The runtime reported an unhealthy orchestration loop and needs attention.";
  }

  if (runtimeStatus === "stopped") {
    return "The automation host is offline until the runtime is started again.";
  }

  return "The runtime is transitioning between lifecycle states.";
}