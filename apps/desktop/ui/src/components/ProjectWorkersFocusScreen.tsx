import { type ReactNode, useState } from "react";

import { Button } from "./Button";
import ScreenScaffold from "./ScreenScaffold";
import { Badge, MetaItem, MetaRow, SummaryStrip, SurfacePanel } from "./Surface";
import { VerticalAccordion } from "./VerticalAccordion";
import type { RuntimeHealthStatus } from "../services/runtime";

export type RuntimeControlAction = "start" | "stop" | "restart";

export type BuildTargetEntry = {
  buildTargetId: number;
  diagnosticMessage: string;
  diagnosticStatus: string;
  name: string;
  unityTargetPlatform: string;
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
  inspectionError: string | null;
  inspectionStale: boolean;
  onBulkInstantCheck: () => void;
  onInstantCheck: (repositoryId: number, repositoryName: string) => void;
  onRestartRuntime: () => void;
  onRetryInventory: () => void;
  onStartRuntime: () => void;
  onStopRuntime: () => void;
  pendingBulkInstantCheck: boolean;
  pendingInstantCheckRepositoryId: number | null;
  pendingRuntimeAction: RuntimeControlAction | null;
  projectWorkers: ProjectWorkerEntry[];
  runtimeStatus: RuntimeHealthStatus | null;
};

export function ProjectWorkersFocusScreen({
  actionError,
  actionMessage,
  inspectionAvailable,
  inspectionError,
  inspectionStale,
  onBulkInstantCheck,
  onInstantCheck,
  onRestartRuntime,
  onRetryInventory,
  onStartRuntime,
  onStopRuntime,
  pendingBulkInstantCheck,
  pendingInstantCheckRepositoryId,
  pendingRuntimeAction,
  projectWorkers,
  runtimeStatus,
}: ProjectWorkersFocusScreenProps) {
  const runtimeIsRunning = runtimeStatus ? runtimeStatus !== "stopped" : false;
  const runtimeBusy = pendingRuntimeAction !== null;
  const instantCheckBusy =
    pendingBulkInstantCheck || pendingInstantCheckRepositoryId !== null;
  const showsInventoryLoading =
    !inspectionAvailable && inspectionError === null;
  const showsInventoryError = !inspectionAvailable && inspectionError !== null;
  const [workerInventoryOpen, setWorkerInventoryOpen] = useState(true);
  const totalBuildTargetCount = projectWorkers.reduce(
    (total, projectWorker) => total + projectWorker.buildTargets.length,
    0,
  );
  const attentionTargetCount = projectWorkers.reduce(
    (total, projectWorker) =>
      total +
      projectWorker.buildTargets.filter(
        (buildTarget) => buildTarget.diagnosticStatus !== "ready",
      ).length,
    0,
  );
  const readyTargetCount = totalBuildTargetCount - attentionTargetCount;

  return (
    <div className="project-workers-focus-shell">
      <ScreenScaffold
        subtitle="Control the local automation runtime and inspect repositories that currently expose enabled build targets."
        eyebrow="Runtime"
        summary={
          <MetaRow>
            <MetaItem label="Runtime">
              {formatRuntimeStatus(runtimeStatus)}
            </MetaItem>
            <MetaItem label="Workers">{projectWorkers.length}</MetaItem>
            <MetaItem label="Targets">{totalBuildTargetCount}</MetaItem>
            <MetaItem label="Attention">
              {attentionTargetCount === 0 ? "All ready" : attentionTargetCount}
            </MetaItem>
          </MetaRow>
        }
        title="Project Workers"
      >
        {actionMessage ? (
          <p className="notice-banner">{actionMessage}</p>
        ) : null}
        {actionError ? (
          <p className="feed-banner feed-banner--error">{actionError}</p>
        ) : null}

        <SurfacePanel
          actions={
            <RuntimeToolbar
              onRestartRuntime={onRestartRuntime}
              onStartRuntime={onStartRuntime}
              onStopRuntime={onStopRuntime}
              pendingRuntimeAction={pendingRuntimeAction}
              runtimeBusy={runtimeBusy}
              runtimeIsRunning={runtimeIsRunning}
            />
          }
          className="project-workers-runtime-panel"
          description="Runtime-wide lifecycle controls stay separate from project worker groups so host state never competes with repository inspection."
          eyebrow="Runtime State"
          headerSeparated
          summary={
            <MetaRow>
              <MetaItem label="State">
                {formatRuntimeStatus(runtimeStatus)}
              </MetaItem>
              <MetaItem label="Controls">
                {runtimeBusy ? "Transition in progress" : "Ready"}
              </MetaItem>
              <MetaItem label="Scope">Local host</MetaItem>
            </MetaRow>
          }
          title="Runtime Controls"
        >
          <div className="project-workers-focus-panel-body">
            <div className="project-workers-focus-status-row">
              <Badge tone={resolveRuntimeBadgeTone(runtimeStatus)}>
                {formatRuntimeStatus(runtimeStatus)}
              </Badge>
              <p className="project-workers-focus-copy">
                {buildRuntimeCopy(runtimeStatus)}
              </p>
            </div>
          </div>
        </SurfacePanel>

        <ProjectWorkersSectionAccordion
          actions={
            inspectionAvailable && projectWorkers.length > 0 ? (
              <Button
                disabled={runtimeBusy || instantCheckBusy}
                leadingIcon="search"
                onClick={onBulkInstantCheck}
                size="sm"
                variant="secondary"
              >
                {pendingBulkInstantCheck
                  ? "Queueing checks..."
                  : "Bulk instant check"}
              </Button>
            ) : null
          }
          description="Repositories that currently expose enabled build targets."
          eyebrow="Inventory"
          onOpenChange={setWorkerInventoryOpen}
          open={workerInventoryOpen}
          summary={
            <MetaRow>
              <MetaItem label="Projects">{projectWorkers.length}</MetaItem>
              <MetaItem label="Ready">{readyTargetCount}</MetaItem>
              <MetaItem label="Attention">
                {attentionTargetCount === 0
                  ? "All ready"
                  : attentionTargetCount}
              </MetaItem>
            </MetaRow>
          }
          title="Worker Inventory"
        >
          {inspectionStale && inspectionError ? (
            <div className="project-workers-focus-state">
              <p className="feed-banner feed-banner--error">
                {inspectionError}
              </p>
              <p className="project-workers-focus-copy">
                Showing the last known worker inventory while the shell recovers
                repository inspection.
              </p>
              <div className="project-workers-focus-state__actions">
                <Button
                  disabled={runtimeBusy}
                  leadingIcon="refresh"
                  onClick={onRetryInventory}
                  size="sm"
                  variant="secondary"
                >
                  Retry inventory
                </Button>
              </div>
            </div>
          ) : null}

          {showsInventoryLoading ? (
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

          {showsInventoryError ? (
            <div className="feed-state project-workers-focus-state">
              <p className="feed-state__title">
                Project worker inventory is unavailable.
              </p>
              <p className="feed-state__copy">{inspectionError}</p>
              <div className="project-workers-focus-state__actions">
                <Button
                  disabled={runtimeBusy}
                  leadingIcon="refresh"
                  onClick={onRetryInventory}
                  size="sm"
                  variant="secondary"
                >
                  Retry inventory
                </Button>
              </div>
            </div>
          ) : null}

          {inspectionAvailable && projectWorkers.length === 0 ? (
            <div className="feed-state">
              <p className="feed-state__title">
                No active project workers configured.
              </p>
              <p className="feed-state__copy">
                Enabled repositories need at least one enabled build target
                before they appear here.
              </p>
            </div>
          ) : null}

          {inspectionAvailable && projectWorkers.length > 0 ? (
            <div className="project-workers-focus-project-list">
              {projectWorkers.map((projectWorker) => (
                <ProjectWorkerAccordion
                  key={projectWorker.repositoryId}
                  instantCheckBusy={instantCheckBusy}
                  onInstantCheck={onInstantCheck}
                  pendingInstantCheckRepositoryId={
                    pendingInstantCheckRepositoryId
                  }
                  projectWorker={projectWorker}
                  runtimeBusy={runtimeBusy}
                />
              ))}
            </div>
          ) : null}
        </ProjectWorkersSectionAccordion>
      </ScreenScaffold>
    </div>
  );
}

function RuntimeToolbar({
  onRestartRuntime,
  onStartRuntime,
  onStopRuntime,
  pendingRuntimeAction,
  runtimeBusy,
  runtimeIsRunning,
}: {
  onRestartRuntime: () => void;
  onStartRuntime: () => void;
  onStopRuntime: () => void;
  pendingRuntimeAction: RuntimeControlAction | null;
  runtimeBusy: boolean;
  runtimeIsRunning: boolean;
}) {
  return (
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
  );
}

function ProjectWorkersSectionAccordion({
  actions,
  children,
  description,
  eyebrow,
  onOpenChange,
  open,
  summary,
  title,
}: {
  actions?: ReactNode;
  children: ReactNode;
  description: string;
  eyebrow: string;
  onOpenChange: (nextOpen: boolean) => void;
  open: boolean;
  summary?: ReactNode;
  title: string;
}) {
  return (
    <VerticalAccordion
      bodyClassName="ui-panel__body project-workers-section-accordion__body"
      className="ui-panel ui-panel--section project-workers-section-accordion"
      collapsedToggleLabel={`Expand ${title}`}
      expandedToggleLabel={`Collapse ${title}`}
      header={
        <div className="project-workers-section-accordion__header-content">
          <div className="ui-panel__title-block">
            <p className="ui-panel__eyebrow">{eyebrow}</p>
            <h2 className="ui-panel__title">{title}</h2>
            <p className="ui-panel__description">{description}</p>
            {summary ? (
              <SummaryStrip className="project-workers-section-accordion__summary">
                {summary}
              </SummaryStrip>
            ) : null}
          </div>
          {actions ? (
            <div className="project-workers-section-accordion__actions">
              {actions}
            </div>
          ) : null}
        </div>
      }
      headerSeparated
      headerClassName="project-workers-section-accordion__header"
      onOpenChange={onOpenChange}
      open={open}
      tone="section"
      triggerMode="button"
    >
      {children}
    </VerticalAccordion>
  );
}

function ProjectWorkerAccordion({
  instantCheckBusy,
  onInstantCheck,
  pendingInstantCheckRepositoryId,
  projectWorker,
  runtimeBusy,
}: {
  instantCheckBusy: boolean;
  onInstantCheck: (repositoryId: number, repositoryName: string) => void;
  pendingInstantCheckRepositoryId: number | null;
  projectWorker: ProjectWorkerEntry;
  runtimeBusy: boolean;
}) {
  const attentionTargetCount = resolveWorkerAttentionCount(projectWorker);
  const readyTargetCount =
    projectWorker.buildTargets.length - attentionTargetCount;

  return (
    <VerticalAccordion
      bodyClassName="project-workers-worker-accordion__body"
      className="ui-panel ui-panel--inset project-workers-worker-accordion"
      collapsedToggleLabel={`Expand ${projectWorker.repositoryName}`}
      defaultOpen={attentionTargetCount > 0}
      expandedToggleLabel={`Collapse ${projectWorker.repositoryName}`}
      header={
        <div className="project-workers-worker-accordion__header-content">
          <div className="ui-panel__title-block">
            <h3 className="ui-panel__title">{projectWorker.repositoryName}</h3>
            <SummaryStrip className="project-workers-worker-accordion__summary">
              <MetaRow>
                <MetaItem label="Poll">
                  {`${projectWorker.pollingIntervalSeconds}s`}
                </MetaItem>
                <MetaItem label="Targets">
                  {projectWorker.buildTargets.length}
                </MetaItem>
                <MetaItem
                  label={attentionTargetCount === 0 ? "Ready" : "Attention"}
                >
                  {attentionTargetCount === 0
                    ? readyTargetCount
                    : attentionTargetCount}
                </MetaItem>
              </MetaRow>
            </SummaryStrip>
          </div>
          <div
            className="project-workers-worker-accordion__actions"
            onClick={(event) => event.stopPropagation()}
            onKeyDown={(event) => event.stopPropagation()}
          >
            <Button
              disabled={runtimeBusy || instantCheckBusy}
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
              {pendingInstantCheckRepositoryId === projectWorker.repositoryId
                ? "Checking..."
                : "Instant Check"}
            </Button>
          </div>
        </div>
      }
      headerSeparated
      headerClassName="project-workers-worker-accordion__header"
      tone="section"
      triggerMode="both"
    >
      <div className="project-workers-focus-target-list">
        {projectWorker.buildTargets.map((buildTarget) => {
          const hasAttention = buildTarget.diagnosticStatus !== "ready";

          return (
            <article
              className="project-workers-focus-target-row"
              key={buildTarget.buildTargetId}
            >
              <div className="project-workers-focus-target-row__main">
                <div className="project-workers-focus-target-row__header">
                  <p className="project-workers-focus-target-row__title">
                    {buildTarget.name}
                  </p>
                  <p className="project-workers-focus-target-row__meta">
                    {buildTarget.unityTargetPlatform}
                  </p>
                </div>

                {hasAttention && buildTarget.diagnosticMessage.trim() ? (
                  <p className="project-workers-focus-target-row__message">
                    {buildTarget.diagnosticMessage}
                  </p>
                ) : null}
              </div>

              <Badge
                tone={resolveBuildTargetBadgeTone(buildTarget.diagnosticStatus)}
              >
                {formatDiagnosticStatus(buildTarget.diagnosticStatus)}
              </Badge>
            </article>
          );
        })}
      </div>
    </VerticalAccordion>
  );
}

function resolveWorkerAttentionCount(projectWorker: ProjectWorkerEntry) {
  return projectWorker.buildTargets.filter(
    (buildTarget) => buildTarget.diagnosticStatus !== "ready",
  ).length;
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

function formatDiagnosticStatus(diagnosticStatus: string) {
  return diagnosticStatus.replace(/_/g, " ");
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
