import { type ReactNode, useState } from "react";

import { type Translate, useLocalization } from "../LocalizationProvider";
import {
  resolveLocalizedProjectAutomationCadenceLabel,
  type ProjectSourceMode,
} from "../projectSourcePresentation";
import { Button } from "./Button";
import ScreenScaffold from "./ScreenScaffold";
import {
  Badge,
  MetaItem,
  MetaRow,
  SummaryStrip,
  SurfacePanel,
} from "./Surface";
import { VerticalAccordion } from "./VerticalAccordion";
import type {
  RuntimeAutomationMode,
  RuntimeHealthStatus,
} from "../services/runtime";

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
  sourceMode: ProjectSourceMode;
  buildTargets: BuildTargetEntry[];
};

type ProjectWorkersFocusScreenProps = {
  actionError: string | null;
  actionMessage: string | null;
  automationMode: RuntimeAutomationMode | null;
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
  automationMode,
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
  const { t } = useLocalization();
  const runtimeIsRunning = runtimeStatus ? runtimeStatus !== "stopped" : false;
  const runtimeBusy = pendingRuntimeAction !== null;
  const instantCheckBusy =
    pendingBulkInstantCheck || pendingInstantCheckRepositoryId !== null;
  const showsInventoryLoading =
    !inspectionAvailable && inspectionError === null;
  const showsInventoryError = !inspectionAvailable && inspectionError !== null;
  const [workerInventoryOpen, setWorkerInventoryOpen] = useState(true);

  return (
    <div className="project-workers-focus-shell">
      <ScreenScaffold title={t("project_workers.title", "Project Workers")}>
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
              t={t}
            />
          }
          className="project-workers-runtime-panel"
          headerSeparated
          title={t("project_workers.runtime_panel.title", "Runtime Controls")}
        >
          <div className="project-workers-focus-panel-body">
            <div
              className="project-workers-focus-status-row"
              title={buildRuntimeCopy(t, runtimeStatus, automationMode)}
            >
              <Badge tone={resolveRuntimeBadgeTone(runtimeStatus)}>
                {formatRuntimeStatus(t, runtimeStatus)}
              </Badge>
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
                title={t(
                  "project_workers.bulk.tooltip",
                  "Queue instant checks for all visible workers.",
                )}
                variant="secondary"
              >
                {pendingBulkInstantCheck
                  ? t("project_workers.bulk.queueing", "Queueing checks...")
                  : t("project_workers.bulk.action", "Bulk instant check")}
              </Button>
            ) : null
          }
          onOpenChange={setWorkerInventoryOpen}
          open={workerInventoryOpen}
          title={t("project_workers.inventory.title", "Worker Inventory")}
          titleTooltip={t(
            "project_workers.inventory.tooltip",
            "Repositories that currently expose enabled build targets.",
          )}
          t={t}
        >
          {inspectionStale && inspectionError ? (
            <div className="project-workers-focus-state">
              <p className="feed-banner feed-banner--error">
                {inspectionError}
              </p>
              <div className="project-workers-focus-state__actions">
                <Button
                  disabled={runtimeBusy}
                  leadingIcon="refresh"
                  onClick={onRetryInventory}
                  size="sm"
                  title={t(
                    "project_workers.inventory.retry_tooltip",
                    "Retry worker inventory inspection.",
                  )}
                  variant="secondary"
                >
                  {t("project_workers.inventory.retry", "Retry inventory")}
                </Button>
              </div>
            </div>
          ) : null}

          {showsInventoryLoading ? (
            <div
              className="feed-state"
              title={t(
                "project_workers.inventory.loading_copy",
                "The shell is refreshing the repositories that currently expose enabled build targets.",
              )}
            >
              <p className="feed-state__title">
                {t(
                  "project_workers.inventory.loading_title",
                  "Loading workers...",
                )}
              </p>
            </div>
          ) : null}

          {showsInventoryError ? (
            <div className="feed-state project-workers-focus-state">
              <p className="feed-state__title">
                {t(
                  "project_workers.inventory.unavailable_title",
                  "Inventory unavailable.",
                )}
              </p>
              <p className="feed-state__copy">{inspectionError}</p>
              <div className="project-workers-focus-state__actions">
                <Button
                  disabled={runtimeBusy}
                  leadingIcon="refresh"
                  onClick={onRetryInventory}
                  size="sm"
                  title={t(
                    "project_workers.inventory.retry_tooltip",
                    "Retry worker inventory inspection.",
                  )}
                  variant="secondary"
                >
                  {t("project_workers.inventory.retry", "Retry inventory")}
                </Button>
              </div>
            </div>
          ) : null}

          {inspectionAvailable && projectWorkers.length === 0 ? (
            <div
              className="feed-state"
              title={t(
                "project_workers.inventory.empty_copy",
                "Enabled repositories need at least one enabled build target before they appear here.",
              )}
            >
              <p className="feed-state__title">
                {t(
                  "project_workers.inventory.empty_title",
                  "No workers available.",
                )}
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
                  t={t}
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
  t,
}: {
  onRestartRuntime: () => void;
  onStartRuntime: () => void;
  onStopRuntime: () => void;
  pendingRuntimeAction: RuntimeControlAction | null;
  runtimeBusy: boolean;
  runtimeIsRunning: boolean;
  t: Translate;
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
        {pendingRuntimeAction === "start"
          ? t("project_workers.runtime.starting", "Starting...")
          : t("project_workers.runtime.start", "Start")}
      </Button>
      <Button
        disabled={runtimeBusy || !runtimeIsRunning}
        onClick={onStopRuntime}
        size="sm"
        variant="ghost"
      >
        {pendingRuntimeAction === "stop"
          ? t("project_workers.runtime.stopping", "Stopping...")
          : t("project_workers.runtime.stop", "Stop")}
      </Button>
      <Button
        disabled={runtimeBusy}
        leadingIcon="refresh"
        onClick={onRestartRuntime}
        size="sm"
        variant="secondary"
      >
        {pendingRuntimeAction === "restart"
          ? t("project_workers.runtime.restarting", "Restarting...")
          : t("project_workers.runtime.restart", "Restart")}
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
  titleTooltip,
  t,
}: {
  actions?: ReactNode;
  children: ReactNode;
  description?: string;
  eyebrow?: string;
  onOpenChange: (nextOpen: boolean) => void;
  open: boolean;
  summary?: ReactNode;
  title: string;
  titleTooltip?: string;
  t: Translate;
}) {
  return (
    <VerticalAccordion
      bodyClassName="ui-panel__body project-workers-section-accordion__body"
      className="ui-panel ui-panel--section project-workers-section-accordion"
      collapsedToggleLabel={t(
        "project_workers.accordion.expand",
        "Expand {{title}}",
        { title },
      )}
      expandedToggleLabel={t(
        "project_workers.accordion.collapse",
        "Collapse {{title}}",
        { title },
      )}
      header={
        <div className="project-workers-section-accordion__header-content">
          <div className="ui-panel__title-block">
            {eyebrow ? <p className="ui-panel__eyebrow">{eyebrow}</p> : null}
            <h2 className="ui-panel__title" title={titleTooltip}>
              {title}
            </h2>
            {description ? (
              <p className="ui-panel__description">{description}</p>
            ) : null}
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
  t,
}: {
  instantCheckBusy: boolean;
  onInstantCheck: (repositoryId: number, repositoryName: string) => void;
  pendingInstantCheckRepositoryId: number | null;
  projectWorker: ProjectWorkerEntry;
  runtimeBusy: boolean;
  t: Translate;
}) {
  const attentionTargetCount = resolveWorkerAttentionCount(projectWorker);
  const readyTargetCount =
    projectWorker.buildTargets.length - attentionTargetCount;

  return (
    <VerticalAccordion
      bodyClassName="project-workers-worker-accordion__body"
      className="ui-panel ui-panel--inset project-workers-worker-accordion"
      collapsedToggleLabel={t(
        "project_workers.accordion.expand",
        "Expand {{title}}",
        { title: projectWorker.repositoryName },
      )}
      defaultOpen={attentionTargetCount > 0}
      expandedToggleLabel={t(
        "project_workers.accordion.collapse",
        "Collapse {{title}}",
        { title: projectWorker.repositoryName },
      )}
      header={
        <div className="project-workers-worker-accordion__header-content">
          <div className="ui-panel__title-block">
            <h3 className="ui-panel__title">{projectWorker.repositoryName}</h3>
            <SummaryStrip className="project-workers-worker-accordion__summary">
              <MetaRow>
                <MetaItem label={t("project_workers.worker.poll", "Poll")}>
                  {resolveLocalizedProjectAutomationCadenceLabel(t, {
                    pollingIntervalSeconds:
                      projectWorker.pollingIntervalSeconds,
                    sourceMode: projectWorker.sourceMode,
                  })}
                </MetaItem>
                <MetaItem
                  label={t("project_workers.worker.targets", "Targets")}
                >
                  {projectWorker.buildTargets.length}
                </MetaItem>
                <MetaItem
                  label={
                    attentionTargetCount === 0
                      ? t("project_workers.worker.ready", "Ready")
                      : t("project_workers.worker.attention", "Attention")
                  }
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
              title={t(
                "project_workers.worker.instant_check_tooltip",
                "Queue an instant check for this worker.",
              )}
              variant="secondary"
            >
              {pendingInstantCheckRepositoryId === projectWorker.repositoryId
                ? t("project_workers.worker.checking", "Checking...")
                : t("project_workers.worker.instant_check", "Instant Check")}
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

function formatRuntimeStatus(
  t: Translate,
  runtimeStatus: RuntimeHealthStatus | null,
) {
  if (!runtimeStatus) {
    return t("app.runtime_status.unavailable", "unavailable");
  }

  switch (runtimeStatus) {
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

function buildRuntimeCopy(
  t: Translate,
  runtimeStatus: RuntimeHealthStatus | null,
  automationMode: RuntimeAutomationMode | null,
) {
  if (!runtimeStatus) {
    return t(
      "project_workers.runtime.copy.unavailable",
      "The shell is still resolving the latest runtime health snapshot.",
    );
  }

  if (runtimeStatus === "healthy" && automationMode === "idle") {
    return t(
      "project_workers.runtime.copy.healthy_idle",
      "The runtime is online, but automatic polling is paused. Manual instant checks remain available.",
    );
  }

  if (runtimeStatus === "healthy") {
    return t(
      "project_workers.runtime.copy.healthy",
      "The runtime is serving the local automation host normally.",
    );
  }

  if (runtimeStatus === "unhealthy") {
    return t(
      "project_workers.runtime.copy.unhealthy",
      "The runtime reported an unhealthy orchestration loop and needs attention.",
    );
  }

  if (runtimeStatus === "stopped") {
    return t(
      "project_workers.runtime.copy.stopped",
      "The automation host is offline until the runtime is started again.",
    );
  }

  return t(
    "project_workers.runtime.copy.transitioning",
    "The runtime is transitioning between lifecycle states.",
  );
}
