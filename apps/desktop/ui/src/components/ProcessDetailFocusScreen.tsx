import {
  startTransition,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";

import { Button } from "./Button";
import {
  Badge,
  FocusPageFrame,
  MetaItem,
  MetaRow,
  SurfacePanel,
} from "./Surface";
import { VerticalAccordion } from "./VerticalAccordion";
import {
  formatProcessFeedBuildCount,
  formatProcessFeedEngineKindBadge,
  formatProcessFeedEngineVersionBadge,
  formatProcessFeedMetaValue,
  formatProcessFeedPublishCount,
  formatProcessFeedStatusLabel,
  normalizeProcessFeedDisplayStatus,
  resolveProcessFeedStatusTone,
  resolveProcessFeedStepDetail,
  resolveProcessFeedStepLabel,
  type ProcessFeedRecord,
} from "./processFeedPresentation";
import {
  deleteReleaseProcessOutputs,
  loadArtifactInspection,
  loadBuildExecutionReport,
  loadBuildHistory,
  openHostPath,
  purgeBuildExecutionRetention,
  readRetainedLogArchiveEntry,
  type ArtifactInspectionRecord,
  type BuildExecutionReportPayload,
  type BuildHistoryRecord,
  type JsonValue,
  type RetainedLogArchiveEntryPreviewPayload,
} from "../services/processDetail";

type ProcessDetailFocusScreenProps = {
  process: ProcessFeedRecord | null;
  usesLiveSnapshot: boolean;
};

type CompletedProcessSnapshot = {
  builds: BuildHistoryRecord[];
  artifacts: ArtifactInspectionRecord[];
  executionReport: BuildExecutionReportPayload | null;
  outputsPath: string | null;
  isLoading: boolean;
  error: string | null;
};

type LogPreviewState = {
  payload: RetainedLogArchiveEntryPreviewPayload | null;
  error: string | null;
};

const EMPTY_COMPLETED_PROCESS_SNAPSHOT: CompletedProcessSnapshot = {
  builds: [],
  artifacts: [],
  executionReport: null,
  outputsPath: null,
  isLoading: false,
  error: null,
};

const DEFAULT_LOG_PREVIEW_MAX_BYTES = 128 * 1024;

export function ProcessDetailFocusScreen({
  process,
  usesLiveSnapshot,
}: ProcessDetailFocusScreenProps) {
  const [completedSnapshot, setCompletedSnapshot] =
    useState<CompletedProcessSnapshot>(EMPTY_COMPLETED_PROCESS_SNAPSHOT);
  const [logPreviewsByEntryPath, setLogPreviewsByEntryPath] = useState<
    Record<string, LogPreviewState>
  >({});
  const [pendingOpenPath, setPendingOpenPath] = useState<string | null>(null);
  const [pendingLogPreviewEntryPath, setPendingLogPreviewEntryPath] = useState<
    string | null
  >(null);
  const [isDeletingOutputs, setIsDeletingOutputs] = useState(false);
  const [isDeletingRetention, setIsDeletingRetention] = useState(false);
  const [deletedOutputs, setDeletedOutputs] = useState(false);
  const [deletedRetention, setDeletedRetention] = useState(false);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const latestCompletedSnapshotRequestIdRef = useRef(0);

  if (!process) {
    return (
      <FocusPageFrame
        description="The selected process is no longer present on the current feed page."
        eyebrow="Process"
        title="Process detail unavailable"
      >
        <SurfacePanel
          description="Return to the main feed and reopen the process if you need a fresh runtime snapshot."
          title="No cached snapshot"
        />
      </FocusPageFrame>
    );
  }

  const normalizedStatus = normalizeProcessFeedDisplayStatus(
    process.display_status,
  );
  const stepLabel = resolveProcessFeedStepLabel(process, normalizedStatus);
  const stepDetail = resolveProcessFeedStepDetail(process);
  const isCompletedMode = isTerminalProcessStatus(normalizedStatus);

  const loadCompletedSnapshot = useEffectEvent(async (releaseRunId: number) => {
    const requestId = latestCompletedSnapshotRequestIdRef.current + 1;
    latestCompletedSnapshotRequestIdRef.current = requestId;

    startTransition(() => {
      setCompletedSnapshot({
        ...EMPTY_COMPLETED_PROCESS_SNAPSHOT,
        isLoading: true,
      });
      setLogPreviewsByEntryPath({});
      setPendingLogPreviewEntryPath(null);
      setDeletedOutputs(false);
      setDeletedRetention(false);
      setActionMessage(null);
      setActionError(null);
    });

    try {
      const [buildHistory, artifactInspection] = await Promise.all([
        loadBuildHistory(),
        loadArtifactInspection(),
      ]);
      const builds = buildHistory.filter(
        (record) => record.release_run_id === releaseRunId,
      );
      const artifacts = artifactInspection.filter(
        (record) => record.release_run_id === releaseRunId,
      );
      const primaryBuild = selectPrimaryBuild(builds);
      const executionReport = primaryBuild
        ? await loadBuildExecutionReport(primaryBuild.build_run_id)
        : null;

      if (requestId !== latestCompletedSnapshotRequestIdRef.current) {
        return;
      }

      startTransition(() => {
        setCompletedSnapshot({
          builds,
          artifacts,
          executionReport,
          outputsPath: resolveOutputsPath(builds, artifacts),
          isLoading: false,
          error: null,
        });
      });
    } catch (error) {
      if (requestId !== latestCompletedSnapshotRequestIdRef.current) {
        return;
      }

      startTransition(() => {
        setCompletedSnapshot({
          ...EMPTY_COMPLETED_PROCESS_SNAPSHOT,
          isLoading: false,
          error: buildProcessDetailErrorMessage(error),
        });
      });
    }
  });

  useEffect(() => {
    if (!isCompletedMode) {
      startTransition(() => {
        setCompletedSnapshot(EMPTY_COMPLETED_PROCESS_SNAPSHOT);
        setLogPreviewsByEntryPath({});
        setPendingLogPreviewEntryPath(null);
        setDeletedOutputs(false);
        setDeletedRetention(false);
        setActionMessage(null);
        setActionError(null);
      });
      return;
    }

    void loadCompletedSnapshot(process.release_run_id);
  }, [isCompletedMode, process.release_run_id]);

  const handleOpenPath = useEffectEvent(async (path: string) => {
    if (!path.trim()) {
      return;
    }

    setPendingOpenPath(path);

    try {
      await openHostPath(path);
      startTransition(() => {
        setActionError(null);
      });
    } catch (error) {
      startTransition(() => {
        setActionError(buildProcessDetailErrorMessage(error));
      });
    } finally {
      startTransition(() => {
        setPendingOpenPath(null);
      });
    }
  });

  const handleLoadLogPreview = useEffectEvent(async (entryPath: string) => {
    const normalizedEntryPath = entryPath.trim();
    if (!normalizedEntryPath) {
      startTransition(() => {
        setLogPreviewsByEntryPath((current) => ({
          ...current,
          [entryPath]: {
            payload: null,
            error:
              "No retained log entry path is currently attached to this process.",
          },
        }));
      });
      return;
    }

    const retentionAnchorBuildRunId =
      completedSnapshot.executionReport?.build_run_id ??
      selectPrimaryBuild(completedSnapshot.builds)?.build_run_id ??
      null;

    if (!retentionAnchorBuildRunId) {
      startTransition(() => {
        setLogPreviewsByEntryPath((current) => ({
          ...current,
          [normalizedEntryPath]: {
            payload: null,
            error:
              "No completed build anchor is available to resolve retained logs for this process.",
          },
        }));
      });
      return;
    }

    setPendingLogPreviewEntryPath(normalizedEntryPath);

    try {
      const preview = await readRetainedLogArchiveEntry(
        retentionAnchorBuildRunId,
        normalizedEntryPath,
        DEFAULT_LOG_PREVIEW_MAX_BYTES,
      );

      startTransition(() => {
        setLogPreviewsByEntryPath((current) => ({
          ...current,
          [normalizedEntryPath]: {
            payload: preview,
            error: null,
          },
        }));
        setActionError(null);
      });
    } catch (error) {
      startTransition(() => {
        setLogPreviewsByEntryPath((current) => ({
          ...current,
          [normalizedEntryPath]: {
            payload: null,
            error: buildProcessDetailErrorMessage(error),
          },
        }));
      });
    } finally {
      startTransition(() => {
        setPendingLogPreviewEntryPath(null);
      });
    }
  });

  const handleDeleteOutputs = useEffectEvent(async () => {
    setIsDeletingOutputs(true);

    try {
      const report = await deleteReleaseProcessOutputs(process.release_run_id);

      startTransition(() => {
        setDeletedOutputs(true);
        setActionError(null);
        setActionMessage(
          report.removed_paths.length > 0
            ? "Process outputs were removed from disk."
            : "Process outputs were already absent from disk.",
        );
      });
    } catch (error) {
      startTransition(() => {
        setActionError(buildProcessDetailErrorMessage(error));
      });
    } finally {
      startTransition(() => {
        setIsDeletingOutputs(false);
      });
    }
  });

  const handleDeleteRetainedMaterial = useEffectEvent(async () => {
    const retentionAnchorBuildRunId =
      completedSnapshot.executionReport?.build_run_id ??
      selectPrimaryBuild(completedSnapshot.builds)?.build_run_id ??
      null;

    if (!retentionAnchorBuildRunId) {
      startTransition(() => {
        setActionError(
          "No completed build anchor is available to remove retained material for this process.",
        );
      });
      return;
    }

    setIsDeletingRetention(true);

    try {
      const report = await purgeBuildExecutionRetention(
        retentionAnchorBuildRunId,
      );

      startTransition(() => {
        setDeletedRetention(true);
        setLogPreviewsByEntryPath({});
        setCompletedSnapshot((current) => ({
          ...current,
          executionReport: current.executionReport
            ? {
                ...current.executionReport,
                exists: false,
                logs_archive_exists: false,
                log_entries: [],
                report: null,
              }
            : current.executionReport,
        }));
        setActionError(null);
        setActionMessage(
          report.removed_paths.length > 0
            ? "Retained report and archived logs were removed from disk."
            : "Retained material was already absent from disk.",
        );
      });
    } catch (error) {
      startTransition(() => {
        setActionError(buildProcessDetailErrorMessage(error));
      });
    } finally {
      startTransition(() => {
        setIsDeletingRetention(false);
      });
    }
  });

  const frameDescription = isCompletedMode
    ? buildCompletedProcessDescription(usesLiveSnapshot)
    : buildOngoingProcessDescription(usesLiveSnapshot);
  const retentionAnchorBuildRunId =
    completedSnapshot.executionReport?.build_run_id ??
    selectPrimaryBuild(completedSnapshot.builds)?.build_run_id ??
    null;
  const retainedDirPath =
    completedSnapshot.executionReport?.retained_dir_path ?? null;
  const logsArchivePath =
    completedSnapshot.executionReport?.logs_archive_path ?? null;
  const retainedLogEntries =
    completedSnapshot.executionReport?.log_entries ?? [];
  const reportSummaryItems = buildExecutionReportSummary(
    completedSnapshot.executionReport?.report ?? null,
  );
  const reportJson = formatJsonValue(
    completedSnapshot.executionReport?.report ?? null,
  );

  return (
    <FocusPageFrame
      className="process-detail-screen"
      description={frameDescription}
      eyebrow={isCompletedMode ? "Process Report" : "Ongoing Process"}
      summary={
        <>
          <Badge tone={resolveProcessFeedStatusTone(normalizedStatus)}>
            {formatProcessFeedStatusLabel(normalizedStatus)}
          </Badge>
          <Badge tone="neutral">{process.git_tag}</Badge>
          <Badge tone="muted">
            {formatProcessFeedEngineKindBadge(process.repository_engine_kind)}
          </Badge>
          <Badge tone="muted">
            {formatProcessFeedEngineVersionBadge(process.engine_version)}
          </Badge>
          <Badge tone="muted">
            {formatProcessFeedBuildCount(process.total_build_runs)}
          </Badge>
          <Badge tone="muted">
            {formatProcessFeedPublishCount(process.total_publish_runs)}
          </Badge>
        </>
      }
      title={`#${process.release_run_id} ${process.repository_name}`}
    >
      {process.error_message ? (
        <p className="feed-banner feed-banner--error">
          {process.error_message}
        </p>
      ) : null}

      {actionError ? (
        <p className="feed-banner feed-banner--error">{actionError}</p>
      ) : null}

      {actionMessage ? (
        <p className="feed-banner feed-banner--success">{actionMessage}</p>
      ) : null}

      {isCompletedMode ? (
        <>
          <SurfacePanel
            bodyClassName="process-detail-panel__body"
            className="process-detail-panel"
            description="Terminal state language sourced from the durable release process snapshot."
            title="Final Outcome"
          >
            <div className="process-detail-panel__step-block">
              <p className="process-detail-panel__step-label">{stepLabel}</p>
              {stepDetail ? (
                <p className="process-detail-panel__step-detail">
                  {stepDetail}
                </p>
              ) : null}
            </div>

            <MetaRow className="process-detail-panel__meta-row">
              <MetaItem label="Terminal state">
                {formatProcessFeedStatusLabel(
                  process.current_step_status || normalizedStatus,
                )}
              </MetaItem>
              <MetaItem label="Builds">
                {formatProcessFeedBuildCount(process.total_build_runs)}
              </MetaItem>
              <MetaItem label="Publishes">
                {formatProcessFeedPublishCount(process.total_publish_runs)}
              </MetaItem>
            </MetaRow>
          </SurfacePanel>

          <SurfacePanel
            bodyClassName="process-detail-panel__body"
            description="Retained execution report data captured after the release process reached a terminal state."
            title="Execution Report"
            tone="inset"
            actions={
              <div className="process-detail-toolbar">
                {completedSnapshot.executionReport?.report_path ? (
                  <Button
                    disabled={
                      deletedRetention ||
                      pendingOpenPath ===
                        completedSnapshot.executionReport.report_path
                    }
                    leadingIcon="arrowUpRight"
                    onClick={() =>
                      void handleOpenPath(
                        completedSnapshot.executionReport?.report_path ?? "",
                      )
                    }
                    size="sm"
                    variant="ghost"
                  >
                    Open report
                  </Button>
                ) : null}
                {retainedDirPath ? (
                  <Button
                    disabled={
                      deletedRetention || pendingOpenPath === retainedDirPath
                    }
                    leadingIcon="folder"
                    onClick={() => void handleOpenPath(retainedDirPath)}
                    size="sm"
                    variant="ghost"
                  >
                    Open retained
                  </Button>
                ) : null}
                {retentionAnchorBuildRunId ? (
                  <Button
                    disabled={deletedRetention || isDeletingRetention}
                    leadingIcon="trash"
                    onClick={() => void handleDeleteRetainedMaterial()}
                    size="sm"
                    variant="ghost"
                  >
                    {isDeletingRetention ? "Deleting..." : "Delete retained"}
                  </Button>
                ) : null}
              </div>
            }
          >
            {completedSnapshot.isLoading ? (
              <p className="process-detail-report__copy">
                Loading retained report data for this completed process...
              </p>
            ) : completedSnapshot.error ? (
              <p className="process-detail-report__copy">
                {completedSnapshot.error}
              </p>
            ) : deletedRetention ? (
              <p className="process-detail-report__copy">
                The retained report directory for this completed process has
                been removed from disk.
              </p>
            ) : completedSnapshot.executionReport?.exists &&
              completedSnapshot.executionReport.report ? (
              <div className="process-detail-report-shell">
                {reportSummaryItems.length > 0 ? (
                  <MetaRow className="process-detail-panel__meta-row">
                    {reportSummaryItems.map((item) => (
                      <MetaItem key={item.label} label={item.label}>
                        {item.value}
                      </MetaItem>
                    ))}
                  </MetaRow>
                ) : null}

                {resolveReportInterruptionMessage(
                  completedSnapshot.executionReport.report,
                ) ? (
                  <p className="process-detail-report__copy">
                    {resolveReportInterruptionMessage(
                      completedSnapshot.executionReport.report,
                    )}
                  </p>
                ) : null}

                <VerticalAccordion
                  bodyInset
                  className="process-detail-log-card"
                  collapsedToggleLabel="Expand retained report JSON"
                  expandedToggleLabel="Collapse retained report JSON"
                  header={
                    <div className="process-detail-log-card__header">
                      <div className="process-detail-log-card__title-block">
                        <p className="process-detail-log-card__title">
                          Raw retained report JSON
                        </p>
                        <p className="process-detail-log-card__copy">
                          Full JSON payload captured under the retained process
                          report file.
                        </p>
                      </div>
                    </div>
                  }
                  headerSeparated
                  tone="section"
                >
                  <pre className="process-detail-log-preview__content process-detail-log-preview__content--json">
                    {reportJson}
                  </pre>
                </VerticalAccordion>
              </div>
            ) : (
              <p className="process-detail-report__copy">
                No retained execution report file was found for this completed
                process.
              </p>
            )}
          </SurfacePanel>

          <SurfacePanel
            bodyClassName="process-detail-panel__body"
            description="Artifacts registered for this process and the shared outputs directory they live under."
            title="Outputs"
            actions={
              <div className="process-detail-toolbar">
                {completedSnapshot.outputsPath ? (
                  <Button
                    disabled={
                      deletedOutputs ||
                      pendingOpenPath === completedSnapshot.outputsPath
                    }
                    leadingIcon="folder"
                    onClick={() =>
                      void handleOpenPath(completedSnapshot.outputsPath ?? "")
                    }
                    size="sm"
                    variant="ghost"
                  >
                    Open outputs
                  </Button>
                ) : null}
                {completedSnapshot.outputsPath ? (
                  <Button
                    disabled={deletedOutputs || isDeletingOutputs}
                    leadingIcon="trash"
                    onClick={() => void handleDeleteOutputs()}
                    size="sm"
                    variant="ghost"
                  >
                    {isDeletingOutputs ? "Deleting..." : "Delete outputs"}
                  </Button>
                ) : null}
              </div>
            }
          >
            <MetaRow className="process-detail-panel__meta-row">
              <MetaItem label="Outputs root">
                {completedSnapshot.outputsPath || "not recorded"}
              </MetaItem>
              <MetaItem label="Artifacts recorded">
                {String(completedSnapshot.artifacts.length)}
              </MetaItem>
            </MetaRow>

            {deletedOutputs ? (
              <p className="process-detail-report__copy">
                The shared outputs directory for this process has been removed
                from disk.
              </p>
            ) : null}

            {completedSnapshot.artifacts.length === 0 ? (
              <p className="process-detail-report__copy">
                No artifact records are currently attached to this process.
              </p>
            ) : (
              <div className="process-detail-artifact-list">
                {completedSnapshot.artifacts.map((artifact) => {
                  const artifactAbsolutePath =
                    resolveArtifactAbsolutePath(artifact);
                  const artifactFolderPath = resolveArtifactFolderPath(artifact);

                  return (
                    <div
                      className="process-detail-artifact-card"
                      key={artifact.artifact_id}
                    >
                      <div className="process-detail-artifact-card__header">
                        <div className="process-detail-artifact-card__title-block">
                          <p className="process-detail-artifact-card__title">
                            {artifact.artifact_name}
                          </p>
                          <p className="process-detail-artifact-card__copy">
                            {artifact.artifact_path}
                          </p>
                          <p className="process-detail-artifact-card__copy process-detail-artifact-card__copy--muted">
                            {formatArtifactActiveLocationSummary(artifact)}
                          </p>
                        </div>

                        <div className="process-detail-toolbar">
                          <Button
                            disabled={
                              deletedOutputs ||
                              !artifactAbsolutePath ||
                              pendingOpenPath === artifactAbsolutePath
                            }
                            leadingIcon="arrowUpRight"
                            onClick={() =>
                              artifactAbsolutePath
                                ? void handleOpenPath(artifactAbsolutePath)
                                : undefined
                            }
                            size="sm"
                            variant="ghost"
                          >
                            Open artifact
                          </Button>
                          <Button
                            disabled={
                              deletedOutputs ||
                              !artifactFolderPath ||
                              pendingOpenPath === artifactFolderPath
                            }
                            leadingIcon="folder"
                            onClick={() =>
                              artifactFolderPath
                                ? void handleOpenPath(artifactFolderPath)
                                : undefined
                            }
                            size="sm"
                            variant="ghost"
                          >
                            Open folder
                          </Button>
                        </div>
                      </div>

                      <MetaRow className="process-detail-panel__meta-row">
                        <MetaItem label="Kind">
                          {artifact.artifact_kind}
                        </MetaItem>
                        <MetaItem label="Build target">
                          {artifact.build_target_name}
                        </MetaItem>
                        <MetaItem label="Active location">
                          {formatArtifactActiveLocationKindLabel(
                            artifact.artifact_active_location_kind,
                          )}
                        </MetaItem>
                        <MetaItem label="Size">
                          {formatByteSize(artifact.size_bytes)}
                        </MetaItem>
                        <MetaItem label="Publishes">
                          {String(artifact.publish_run_count)}
                        </MetaItem>
                      </MetaRow>

                      {artifact.publish_runs.length > 0 ? (
                        <div className="project-detail-status-grid">
                          {artifact.publish_runs.map((publishRun) => (
                            <div
                              className="project-detail-target-card"
                              key={publishRun.publish_run_id}
                            >
                              <div className="project-detail-target-card__header">
                                <div className="project-detail-target-card__title-block">
                                  <h4 className="project-detail-target-card__title">
                                    {publishRun.publish_target_name}
                                  </h4>
                                  <p className="project-detail-target-card__copy">
                                    {formatArtifactPublishRunSummary(
                                      publishRun,
                                    )}
                                  </p>
                                </div>

                                <div className="project-detail-target-card__badges">
                                  <Badge
                                    tone={resolveArtifactPublishRunTone(
                                      publishRun.status,
                                    )}
                                  >
                                    {publishRun.status}
                                  </Badge>
                                  <Badge tone="neutral">
                                    {formatPublishTargetKindLabel(
                                      publishRun.publish_target_kind,
                                    )}
                                  </Badge>
                                </div>
                              </div>
                            </div>
                          ))}
                        </div>
                      ) : null}
                    </div>
                  );
                })}
              </div>
            )}
          </SurfacePanel>

          <SurfacePanel
            bodyClassName="process-detail-panel__body"
            description="Archived execution log entries stored under retained/execution-logs.zip for this completed process."
            title="Retained Logs"
            actions={
              logsArchivePath ? (
                <Button
                  disabled={
                    deletedRetention || pendingOpenPath === logsArchivePath
                  }
                  leadingIcon="arrowUpRight"
                  onClick={() => void handleOpenPath(logsArchivePath)}
                  size="sm"
                  variant="ghost"
                >
                  Open log archive
                </Button>
              ) : null
            }
          >
            <MetaRow className="process-detail-panel__meta-row">
              <MetaItem label="Archive path">
                {logsArchivePath || "not recorded"}
              </MetaItem>
              <MetaItem label="Entries">
                {String(retainedLogEntries.length)}
              </MetaItem>
            </MetaRow>

            {deletedRetention ? (
              <p className="process-detail-report__copy">
                The retained log archive for this completed process has been
                removed from disk.
              </p>
            ) : !completedSnapshot.executionReport?.logs_archive_exists ? (
              <p className="process-detail-report__copy">
                No retained log archive was found for this completed process.
              </p>
            ) : retainedLogEntries.length === 0 ? (
              <p className="process-detail-report__copy">
                The retained log archive exists, but it does not contain any
                readable log entries.
              </p>
            ) : (
              <div className="process-detail-log-list">
                {retainedLogEntries.map((entry) => {
                  const logPreview = logPreviewsByEntryPath[entry.entry_path];

                  return (
                    <VerticalAccordion
                      bodyInset
                      className="process-detail-log-card"
                      collapsedToggleLabel={`Expand retained log ${entry.entry_name}`}
                      expandedToggleLabel={`Collapse retained log ${entry.entry_name}`}
                      header={
                        <div className="process-detail-log-card__header">
                          <div className="process-detail-log-card__title-block">
                            <p className="process-detail-log-card__title">
                              {entry.entry_name}
                            </p>
                            <p className="process-detail-log-card__copy">
                              {entry.entry_path}
                            </p>
                          </div>

                          <div className="process-detail-log-card__badges">
                            <Badge tone="muted">
                              {formatByteSize(entry.size_bytes)}
                            </Badge>
                            <Badge tone="muted">
                              {formatByteSize(entry.compressed_size_bytes)}{" "}
                              zipped
                            </Badge>
                          </div>
                        </div>
                      }
                      headerSeparated
                      tone="section"
                      key={entry.entry_path}
                    >
                      <div className="process-detail-log-card__body">
                        <div className="process-detail-toolbar">
                          <Button
                            disabled={
                              deletedRetention ||
                              pendingLogPreviewEntryPath === entry.entry_path
                            }
                            leadingIcon="terminal"
                            onClick={() =>
                              void handleLoadLogPreview(entry.entry_path)
                            }
                            size="sm"
                            variant="ghost"
                          >
                            {pendingLogPreviewEntryPath === entry.entry_path
                              ? "Loading preview..."
                              : "Read entry"}
                          </Button>
                        </div>

                        <MetaRow className="process-detail-panel__meta-row">
                          <MetaItem label="Archive path">
                            {logsArchivePath || "not recorded"}
                          </MetaItem>
                          <MetaItem label="Expanded size">
                            {formatByteSize(entry.size_bytes)}
                          </MetaItem>
                          <MetaItem label="Compressed size">
                            {formatByteSize(entry.compressed_size_bytes)}
                          </MetaItem>
                        </MetaRow>

                        {logPreview?.error ? (
                          <p className="process-detail-report__copy">
                            {logPreview.error}
                          </p>
                        ) : null}

                        {logPreview?.payload ? (
                          <div className="process-detail-log-preview">
                            <p className="process-detail-log-preview__meta">
                              {logPreview.payload.truncated
                                ? `Showing the last ${formatByteSize(DEFAULT_LOG_PREVIEW_MAX_BYTES)} of ${formatByteSize(logPreview.payload.size_bytes)}.`
                                : `Showing the full log file (${formatByteSize(logPreview.payload.size_bytes)}).`}
                            </p>
                            <pre className="process-detail-log-preview__content">
                              {logPreview.payload.content}
                            </pre>
                          </div>
                        ) : null}
                      </div>
                    </VerticalAccordion>
                  );
                })}
              </div>
            )}
          </SurfacePanel>
        </>
      ) : (
        <SurfacePanel
          bodyClassName="process-detail-panel__body"
          className="process-detail-panel"
          description="Short operator-facing progress language sourced from the runtime process feed."
          title="Current Step"
        >
          <div className="process-detail-panel__step-block">
            <p className="process-detail-panel__step-label">{stepLabel}</p>
            {stepDetail ? (
              <p className="process-detail-panel__step-detail">{stepDetail}</p>
            ) : null}
          </div>

          <MetaRow className="process-detail-panel__meta-row">
            <MetaItem label="Step state">
              {formatProcessFeedStatusLabel(
                process.current_step_status || normalizedStatus,
              )}
            </MetaItem>
            <MetaItem label="Builds">
              {formatProcessFeedBuildCount(process.total_build_runs)}
            </MetaItem>
            <MetaItem label="Publishes">
              {formatProcessFeedPublishCount(process.total_publish_runs)}
            </MetaItem>
          </MetaRow>
        </SurfacePanel>
      )}

      <SurfacePanel
        bodyClassName="process-detail-panel__body"
        description="Durable runtime timestamps and identifiers currently attached to this release process."
        title="Runtime Metadata"
        tone="inset"
      >
        <MetaRow className="process-detail-panel__meta-row">
          <MetaItem label="Started">
            {formatProcessFeedMetaValue(process.started_at, "not started")}
          </MetaItem>
          <MetaItem label="Finished">
            {formatProcessFeedMetaValue(process.finished_at, "still active")}
          </MetaItem>
          <MetaItem label="Updated">
            {formatProcessFeedMetaValue(process.updated_at)}
          </MetaItem>
        </MetaRow>

        <MetaRow className="process-detail-panel__meta-row">
          <MetaItem label="Commit">
            {formatProcessFeedMetaValue(process.git_commit, "pending")}
          </MetaItem>
          <MetaItem label="Created">
            {formatProcessFeedMetaValue(process.created_at)}
          </MetaItem>
          <MetaItem label="Repository">{process.repository_url}</MetaItem>
        </MetaRow>
      </SurfacePanel>
    </FocusPageFrame>
  );
}

function isTerminalProcessStatus(status: string) {
  const normalizedStatus = normalizeProcessFeedDisplayStatus(status);
  return (
    normalizedStatus === "succeeded" ||
    normalizedStatus === "failed" ||
    normalizedStatus === "canceled"
  );
}

function buildOngoingProcessDescription(usesLiveSnapshot: boolean) {
  return usesLiveSnapshot
    ? "The shell is rendering the latest runtime snapshot for an ongoing process."
    : "The shell is rendering the last cached snapshot for an ongoing process because this release is no longer visible on the current feed page.";
}

function buildCompletedProcessDescription(usesLiveSnapshot: boolean) {
  return usesLiveSnapshot
    ? "The shell is rendering the durable report view for a completed process together with its retained outputs and logs."
    : "The shell is rendering the last cached completed-process snapshot together with durable report data because this release is no longer visible on the current feed page.";
}

function selectPrimaryBuild(builds: BuildHistoryRecord[]) {
  return builds[0] ?? null;
}

function resolveOutputsPath(
  builds: BuildHistoryRecord[],
  artifacts: ArtifactInspectionRecord[],
) {
  const buildOutputsPath = builds.find((record) =>
    record.artifact_root_path?.trim(),
  )?.artifact_root_path;

  if (buildOutputsPath?.trim()) {
    return buildOutputsPath;
  }

  return (
    artifacts.find((record) => record.artifact_root_path?.trim())
      ?.artifact_root_path ?? null
  );
}

function resolveArtifactAbsolutePath(artifact: ArtifactInspectionRecord) {
  if (
    artifact.artifact_active_location_kind === "filesystem_absolute" &&
    looksLikeAbsoluteHostPath(artifact.artifact_active_location_ref)
  ) {
    return artifact.artifact_active_location_ref;
  }

  if (looksLikeAbsoluteHostPath(artifact.artifact_active_location_ref)) {
    return artifact.artifact_active_location_ref;
  }

  if (!artifact.artifact_root_path?.trim()) {
    return null;
  }

  const separator = artifact.artifact_root_path.includes("\\") ? "\\" : "/";
  const normalizedRoot = artifact.artifact_root_path.replace(/[\\/]+$/, "");
  const normalizedRelative = artifact.artifact_path.replace(
    /[\\/]+/g,
    separator,
  );
  return `${normalizedRoot}${separator}${normalizedRelative}`;
}

function resolveArtifactFolderPath(artifact: ArtifactInspectionRecord) {
  const artifactAbsolutePath = resolveArtifactAbsolutePath(artifact);
  if (artifactAbsolutePath?.trim()) {
    return artifactAbsolutePath.replace(/[\\/][^\\/]+$/, "");
  }

  return artifact.artifact_root_path?.trim() || null;
}

function formatArtifactActiveLocationSummary(artifact: ArtifactInspectionRecord) {
  return `${formatArtifactActiveLocationKindLabel(
    artifact.artifact_active_location_kind,
  )}: ${artifact.artifact_active_location_ref}`;
}

function formatArtifactActiveLocationKindLabel(kind: string) {
  switch (kind.trim().toLocaleLowerCase()) {
    case "runtime_artifact":
      return "Managed output root";
    case "filesystem_absolute":
      return "Filesystem publish move";
    default:
      return kind.replace(/_/g, " ");
  }
}

function formatArtifactPublishRunSummary(
  publishRun: ArtifactInspectionRecord["publish_runs"][number],
) {
  if (publishRun.destination_ref?.trim()) {
    return publishRun.destination_ref;
  }

  return "Destination reference pending.";
}

function resolveArtifactPublishRunTone(status: string): "strong" | "neutral" | "muted" {
  switch (status.trim().toLocaleLowerCase()) {
    case "succeeded":
      return "strong";
    case "failed":
    case "running":
      return "neutral";
    default:
      return "muted";
  }
}

function formatPublishTargetKindLabel(kind: string) {
  switch (kind.trim().toLocaleLowerCase()) {
    case "filesystem":
      return "Move To Folder";
    case "itch":
      return "Itch.io Upload";
    default:
      return kind;
  }
}

function looksLikeAbsoluteHostPath(value: string) {
  return (
    /^[a-zA-Z]:[\\/]/.test(value) ||
    value.startsWith("/") ||
    value.startsWith("\\\\")
  );
}

function buildExecutionReportSummary(report: JsonValue | null) {
  const items = [
    buildSummaryItem("Schema", readJsonNumber(report, ["schema_version"])),
    buildSummaryItem(
      "Build state",
      readJsonString(report, ["build_run", "status"]),
    ),
    buildSummaryItem("Cleanup", readJsonString(report, ["cleanup", "status"])),
    buildSummaryItem("Trigger", readJsonString(report, ["cleanup", "trigger"])),
    buildSummaryItem("Attempts", readJsonArrayLength(report, ["attempts"])),
    buildSummaryItem("Stages", readJsonArrayLength(report, ["stages"])),
  ];

  return items.filter(
    (item): item is { label: string; value: string } => item !== null,
  );
}

function resolveReportInterruptionMessage(report: JsonValue | null) {
  const kind = readJsonString(report, ["interruption", "kind"]);
  const message = readJsonString(report, ["interruption", "message"]);

  if (!kind && !message) {
    return null;
  }

  if (kind && message) {
    return `Interruption: ${kind} · ${message}`;
  }

  return kind || message;
}

function buildSummaryItem(label: string, value: string | null) {
  if (!value) {
    return null;
  }

  return { label, value };
}

function readJsonNumber(report: JsonValue | null, path: string[]) {
  const value = readJsonValue(report, path);
  if (typeof value === "number") {
    return String(value);
  }

  return null;
}

function readJsonString(report: JsonValue | null, path: string[]) {
  const value = readJsonValue(report, path);
  return typeof value === "string" && value.trim() ? value : null;
}

function readJsonArrayLength(report: JsonValue | null, path: string[]) {
  const value = readJsonValue(report, path);
  return Array.isArray(value) ? String(value.length) : null;
}

function readJsonValue(
  report: JsonValue | null,
  path: string[],
): JsonValue | null {
  let current: JsonValue | null = report;

  for (const segment of path) {
    if (!current || Array.isArray(current) || typeof current !== "object") {
      return null;
    }

    current = current[segment] ?? null;
  }

  return current;
}

function formatJsonValue(value: JsonValue | null) {
  if (value === null) {
    return "null";
  }

  return JSON.stringify(value, null, 2);
}

function formatByteSize(sizeBytes: number | null) {
  if (sizeBytes === null || Number.isNaN(sizeBytes)) {
    return "unknown";
  }

  if (sizeBytes < 1024) {
    return `${sizeBytes} B`;
  }

  const units = ["KB", "MB", "GB", "TB"];
  let value = sizeBytes / 1024;
  let unitIndex = 0;

  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unitIndex]}`;
}

function buildProcessDetailErrorMessage(error: unknown) {
  if (typeof error === "string" && error.trim()) {
    return error;
  }

  if (error instanceof Error && error.message.trim()) {
    return error.message;
  }

  return "The desktop shell could not load process report data.";
}
