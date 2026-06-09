import {
  startTransition,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";

import { Button } from "./Button";
import { ArtifactViewer } from "./ArtifactViewer";
import { ConfirmDialog } from "./ConfirmDialog";
import { ExecutionReportPanel } from "./ExecutionReportPanel";
import { LogViewerModal } from "./LogViewerModal";
import { OutputsPanel } from "./OutputsPanel";
import { useOverlay } from "./OverlayManager";
import { RetainedLogsPanel } from "./RetainedLogsPanel";
import ScreenScaffold from "./ScreenScaffold";
import { Badge, MetaItem, MetaRow, SurfacePanel } from "./Surface";
import {
  formatLocalizedProcessFeedBuildCount,
  formatLocalizedProcessFeedEngineKindBadge,
  formatLocalizedProcessFeedEngineVersionBadge,
  formatLocalizedProcessFeedMetaValue,
  formatLocalizedProcessFeedPublishCount,
  formatLocalizedProcessFeedStatusLabel,
  normalizeProcessFeedDisplayStatus,
  resolveProcessFeedStatusTone,
  resolveProcessFeedStepDetail,
  resolveLocalizedProcessFeedStepLabel,
  type ProcessFeedRecord,
} from "./processFeedPresentation";
import { type Translate, useLocalization } from "../LocalizationProvider";
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
  onRequestCancel?: (process: ProcessFeedRecord) => Promise<void>;
  onRequestRerun?: (process: ProcessFeedRecord) => Promise<void>;
};

type CompletedProcessSnapshot = {
  releaseRunId: number | null;
  builds: BuildHistoryRecord[];
  artifacts: ArtifactInspectionRecord[];
  executionReport: BuildExecutionReportPayload | null;
  outputsPath: string | null;
  isAvailable: boolean;
  isLoading: boolean;
  isRefreshing: boolean;
  isStale: boolean;
  error: string | null;
};

type LogPreviewState = {
  payload: RetainedLogArchiveEntryPreviewPayload | null;
  error: string | null;
};

const EMPTY_COMPLETED_PROCESS_SNAPSHOT: CompletedProcessSnapshot = {
  releaseRunId: null,
  builds: [],
  artifacts: [],
  executionReport: null,
  outputsPath: null,
  isAvailable: false,
  isLoading: false,
  isRefreshing: false,
  isStale: false,
  error: null,
};

const DEFAULT_LOG_PREVIEW_MAX_BYTES = 128 * 1024;

export function ProcessDetailFocusScreen({
  process,
  usesLiveSnapshot,
  onRequestCancel,
  onRequestRerun,
}: ProcessDetailFocusScreenProps) {
  const { openOverlay } = useOverlay();
  const { t } = useLocalization();
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
  const [isCancelingProcess, setIsCancelingProcess] = useState(false);
  const [isRequestingRerun, setIsRequestingRerun] = useState(false);
  const [deletedOutputs, setDeletedOutputs] = useState(false);
  const [deletedRetention, setDeletedRetention] = useState(false);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const latestCompletedSnapshotRequestIdRef = useRef(0);

  if (!process) {
    return (
      <ScreenScaffold
        subtitle={t(
          "process_detail.unavailable.subtitle",
          "The selected process is no longer present on the current feed page.",
        )}
        eyebrow={t("process_detail.unavailable.eyebrow", "Process")}
        title={t(
          "process_detail.unavailable.title",
          "Process detail unavailable",
        )}
      >
        <SurfacePanel
          description={t(
            "process_detail.unavailable.panel.description",
            "Return to the main feed and reopen the process if you need a fresh runtime snapshot.",
          )}
          title={t(
            "process_detail.unavailable.panel.title",
            "No cached snapshot",
          )}
        />
      </ScreenScaffold>
    );
  }

  const normalizedStatus = normalizeProcessFeedDisplayStatus(
    process.display_status,
  );
  const stepLabel = resolveLocalizedProcessFeedStepLabel(
    t,
    process,
    normalizedStatus,
  );
  const stepDetail = resolveProcessFeedStepDetail(process);
  const isCompletedMode = isTerminalProcessStatus(normalizedStatus);
  const isOnHold = isProcessOnHold(process);
  const canRequestCancel = !isCompletedMode;

  const loadCompletedSnapshot = useEffectEvent(async (releaseRunId: number) => {
    const requestId = latestCompletedSnapshotRequestIdRef.current + 1;
    latestCompletedSnapshotRequestIdRef.current = requestId;

    startTransition(() => {
      setCompletedSnapshot((current) => {
        if (current.isAvailable && current.releaseRunId === releaseRunId) {
          return {
            ...current,
            isRefreshing: true,
            isStale: false,
            error: null,
          };
        }

        return {
          ...EMPTY_COMPLETED_PROCESS_SNAPSHOT,
          releaseRunId,
          isLoading: true,
        };
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
          releaseRunId,
          builds,
          artifacts,
          executionReport,
          outputsPath: resolveOutputsPath(builds, artifacts),
          isAvailable: true,
          isLoading: false,
          isRefreshing: false,
          isStale: false,
          error: null,
        });
      });
    } catch (error) {
      if (requestId !== latestCompletedSnapshotRequestIdRef.current) {
        return;
      }

      startTransition(() => {
        setCompletedSnapshot((current) => {
          if (current.isAvailable && current.releaseRunId === releaseRunId) {
            return {
              ...current,
              isLoading: false,
              isRefreshing: false,
              isStale: true,
              error: buildProcessDetailErrorMessage(t, error),
            };
          }

          return {
            ...EMPTY_COMPLETED_PROCESS_SNAPSHOT,
            releaseRunId,
            isLoading: false,
            error: buildProcessDetailErrorMessage(t, error),
          };
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

  const handleRetryCompletedSnapshot = useEffectEvent(() => {
    if (!isCompletedMode) {
      return;
    }

    void loadCompletedSnapshot(process.release_run_id);
  });

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
        setActionError(buildProcessDetailErrorMessage(t, error));
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
            error: t(
              "process_detail.logs.missing_entry_path",
              "No retained log entry path is currently attached to this process.",
            ),
          },
        }));
      });
      return null;
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
            error: t(
              "process_detail.logs.missing_build_anchor",
              "No completed build anchor is available to resolve retained logs for this process.",
            ),
          },
        }));
      });
      return null;
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
      return preview;
    } catch (error) {
      startTransition(() => {
        setLogPreviewsByEntryPath((current) => ({
          ...current,
          [normalizedEntryPath]: {
            payload: null,
            error: buildProcessDetailErrorMessage(t, error),
          },
        }));
      });
      return null;
    } finally {
      startTransition(() => {
        setPendingLogPreviewEntryPath(null);
      });
    }
  });

  const handleOpenReportViewer = useEffectEvent(async () => {
    if (!completedSnapshot.executionReport?.report) {
      return;
    }

    await openOverlay(LogViewerModal, {
      content: reportJson,
      description: t(
        "process_detail.report.viewer.description",
        "Full retained JSON captured for this completed process. Use the viewer controls to switch between wrapped and preserved line layout.",
      ),
      downloadFileName: resolveLogViewerFileName(
        completedSnapshot.executionReport.report_path,
        "retained-report.json",
      ),
      initialWrap: false,
      meta: completedSnapshot.executionReport.report_path
        ? t("process_detail.report.viewer.path_meta", "Report path: {{path}}", {
            path: completedSnapshot.executionReport.report_path,
          })
        : undefined,
      title: t("process_detail.report.viewer.title", "Retained report JSON"),
    });
  });

  const handleOpenRetainedLogViewer = useEffectEvent(
    async (entryPath: string, entryName: string) => {
      const normalizedEntryPath = entryPath.trim();

      if (!normalizedEntryPath) {
        return;
      }

      const preview =
        logPreviewsByEntryPath[normalizedEntryPath]?.payload ??
        (await handleLoadLogPreview(normalizedEntryPath));

      if (!preview) {
        return;
      }

      await openOverlay(LogViewerModal, {
        content: preview.content,
        description: t(
          "process_detail.logs.viewer.description",
          "Retained log content loaded from the durable execution archive.",
        ),
        downloadFileName: resolveLogViewerFileName(
          entryName,
          "retained-log.txt",
        ),
        initialWrap: false,
        meta: buildRetainedLogViewerMeta(t, preview),
        title: entryName,
      });
    },
  );

  const handleRequestDeleteOutputs = useEffectEvent(async () => {
    const shouldDelete = await openOverlay<boolean>(ConfirmDialog, {
      cancelLabel: t(
        "process_detail.confirm.delete_outputs.cancel",
        "Keep outputs",
      ),
      confirmLabel: t(
        "process_detail.confirm.delete_outputs.confirm",
        "Delete outputs",
      ),
      description: t(
        "process_detail.confirm.delete_outputs.description",
        "This removes the shared outputs directory currently attached to the selected process.",
      ),
      message: t(
        "process_detail.confirm.delete_outputs.message",
        "Use this only when you want to remove the current process outputs from disk. Published destinations and database records are not rewritten by this action.",
      ),
      title: t(
        "process_detail.confirm.delete_outputs.title",
        "Delete outputs?",
      ),
    });

    if (!shouldDelete) {
      return;
    }

    await handleDeleteOutputs();
  });

  const handleRequestDeleteRetainedMaterial = useEffectEvent(async () => {
    const shouldDelete = await openOverlay<boolean>(ConfirmDialog, {
      cancelLabel: t(
        "process_detail.confirm.delete_retained.cancel",
        "Keep retained material",
      ),
      confirmLabel: t(
        "process_detail.confirm.delete_retained.confirm",
        "Delete retained material",
      ),
      description: t(
        "process_detail.confirm.delete_retained.description",
        "This removes the retained execution report and archived logs currently attached to the completed process.",
      ),
      message: t(
        "process_detail.confirm.delete_retained.message",
        "Use this only when you want to discard durable retained execution material from disk. The runtime history record remains, but the retained files are removed.",
      ),
      title: t(
        "process_detail.confirm.delete_retained.title",
        "Delete retained material?",
      ),
    });

    if (!shouldDelete) {
      return;
    }

    await handleDeleteRetainedMaterial();
  });

  const handleRequestRerun = useEffectEvent(async () => {
    if (!onRequestRerun) {
      return;
    }

    const shouldRerun = await openOverlay<boolean>(ConfirmDialog, {
      cancelLabel: t("process_detail.confirm.rerun.cancel", "Keep current run"),
      confirmLabel: t("process_detail.confirm.rerun.confirm", "Rerun process"),
      description: t(
        "process_detail.confirm.rerun.description",
        "This requeues the selected release process using the same repository and tag.",
      ),
      message: t(
        "process_detail.confirm.rerun.message",
        "HGP will clear the derived build and publish state for this release, return to the main feed, and queue the process again.",
      ),
      title: t("process_detail.confirm.rerun.title", "Rerun process?"),
    });

    if (!shouldRerun) {
      return;
    }

    setIsRequestingRerun(true);

    try {
      startTransition(() => {
        setActionError(null);
        setActionMessage(null);
      });

      await onRequestRerun(process);
    } catch (error) {
      startTransition(() => {
        setActionError(buildProcessDetailErrorMessage(t, error));
      });
    } finally {
      startTransition(() => {
        setIsRequestingRerun(false);
      });
    }
  });

  const handleRequestCancel = useEffectEvent(async () => {
    if (!onRequestCancel || isCompletedMode) {
      return;
    }

    const shouldCancel = await openOverlay<boolean>(ConfirmDialog, {
      cancelLabel: t(
        "process_detail.confirm.cancel.cancel",
        "Keep process running",
      ),
      confirmLabel: t(
        "process_detail.confirm.cancel.confirm",
        "Interrupt process",
      ),
      description: t(
        "process_detail.confirm.cancel.description",
        "This interrupts the active process, finalizes the current logs, and runs cleanup for any in-flight build or publish work.",
      ),
      message: t(
        "process_detail.confirm.cancel.message",
        "Use this to stop the current process immediately. HGP will interrupt active child processes, write the final logs, and clean the current workspace before the runtime settles on the canceled state.",
      ),
      title: t("process_detail.confirm.cancel.title", "Interrupt process?"),
    });

    if (!shouldCancel) {
      return;
    }

    setIsCancelingProcess(true);

    try {
      startTransition(() => {
        setActionError(null);
        setActionMessage(null);
      });

      await onRequestCancel(process);

      startTransition(() => {
        setActionMessage(
          t(
            "process_detail.actions.cancel_requested",
            "Interrupt request accepted. The process feed will refresh as soon as the runtime snapshot advances.",
          ),
        );
      });
    } catch (error) {
      startTransition(() => {
        setActionError(buildProcessDetailErrorMessage(t, error));
      });
    } finally {
      startTransition(() => {
        setIsCancelingProcess(false);
      });
    }
  });

  const handleOpenArtifactViewer = useEffectEvent(
    async (artifact: ArtifactInspectionRecord) => {
      const artifactAbsolutePath = resolveArtifactAbsolutePath(artifact);
      const artifactFolderPath = resolveArtifactFolderPath(artifact);

      await openOverlay(ArtifactViewer, {
        artifact,
        artifactAbsolutePath,
        artifactFolderPath,
        artifactLocationSummary: formatArtifactActiveLocationSummary(
          t,
          artifact,
        ),
        onOpenArtifact: artifactAbsolutePath
          ? () => void handleOpenPath(artifactAbsolutePath)
          : undefined,
        onOpenFolder: artifactFolderPath
          ? () => void handleOpenPath(artifactFolderPath)
          : undefined,
        openArtifactDisabled:
          deletedOutputs ||
          !artifactAbsolutePath ||
          pendingOpenPath === artifactAbsolutePath,
        openArtifactLabel:
          artifactAbsolutePath && pendingOpenPath === artifactAbsolutePath
            ? t(
                "process_detail.artifact_viewer.opening_artifact",
                "Opening artifact...",
              )
            : t(
                "process_detail.artifact_viewer.open_artifact",
                "Open artifact",
              ),
        openFolderDisabled:
          deletedOutputs ||
          !artifactFolderPath ||
          pendingOpenPath === artifactFolderPath,
        openFolderLabel:
          artifactFolderPath && pendingOpenPath === artifactFolderPath
            ? t(
                "process_detail.artifact_viewer.opening_folder",
                "Opening folder...",
              )
            : t("process_detail.artifact_viewer.open_folder", "Open folder"),
        resolvePublishTargetKindTone,
      });
    },
  );

  const handleDeleteOutputs = useEffectEvent(async () => {
    setIsDeletingOutputs(true);

    try {
      const report = await deleteReleaseProcessOutputs(process.release_run_id);

      startTransition(() => {
        setDeletedOutputs(true);
        setActionError(null);
        setActionMessage(
          report.removed_paths.length > 0
            ? t(
                "process_detail.actions.outputs_removed",
                "Process outputs were removed from disk.",
              )
            : t(
                "process_detail.actions.outputs_absent",
                "Process outputs were already absent from disk.",
              ),
        );
      });
    } catch (error) {
      startTransition(() => {
        setActionError(buildProcessDetailErrorMessage(t, error));
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
          t(
            "process_detail.actions.missing_build_anchor_remove_retained",
            "No completed build anchor is available to remove retained material for this process.",
          ),
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
            ? t(
                "process_detail.actions.retained_removed",
                "Retained report and archived logs were removed from disk.",
              )
            : t(
                "process_detail.actions.retained_absent",
                "Retained material was already absent from disk.",
              ),
        );
      });
    } catch (error) {
      startTransition(() => {
        setActionError(buildProcessDetailErrorMessage(t, error));
      });
    } finally {
      startTransition(() => {
        setIsDeletingRetention(false);
      });
    }
  });

  const frameDescription = isCompletedMode
    ? buildCompletedProcessDescription(t, usesLiveSnapshot)
    : buildOngoingProcessDescription(t, usesLiveSnapshot);
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
    t,
    completedSnapshot.executionReport?.report ?? null,
  );
  const reportJson = formatJsonValue(
    completedSnapshot.executionReport?.report ?? null,
  );
  const reportInterruptionMessage = resolveReportInterruptionMessage(
    t,
    completedSnapshot.executionReport?.report ?? null,
  );
  const showsCompletedSnapshotLoading =
    completedSnapshot.isLoading && !completedSnapshot.isAvailable;
  const showsCompletedSnapshotUnavailable =
    !completedSnapshot.isLoading &&
    !completedSnapshot.isAvailable &&
    completedSnapshot.error !== null;

  return (
    <ScreenScaffold
      className="process-detail-screen"
      subtitle={frameDescription}
      eyebrow={
        isCompletedMode
          ? t("process_detail.eyebrow.completed", "Process Report")
          : t("process_detail.eyebrow.ongoing", "Ongoing Process")
      }
      summary={
        <>
          <Badge tone={resolveProcessFeedStatusTone(normalizedStatus)}>
            {formatLocalizedProcessFeedStatusLabel(t, normalizedStatus)}
          </Badge>
          <Badge tone="neutral">{process.git_tag}</Badge>
          <Badge tone="muted">
            {formatLocalizedProcessFeedEngineKindBadge(
              t,
              process.repository_engine_kind,
            )}
          </Badge>
          <Badge tone="muted">
            {formatLocalizedProcessFeedEngineVersionBadge(
              t,
              process.engine_version,
            )}
          </Badge>
          <Badge tone="muted">
            {formatLocalizedProcessFeedBuildCount(t, process.total_build_runs)}
          </Badge>
          <Badge tone="muted">
            {formatLocalizedProcessFeedPublishCount(
              t,
              process.total_publish_runs,
            )}
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

      {isCompletedMode &&
      completedSnapshot.isStale &&
      completedSnapshot.error ? (
        <>
          <p className="feed-banner feed-banner--error">
            {completedSnapshot.error}
          </p>
          <p className="process-detail-report__copy">
            {t(
              "process_detail.stale_copy",
              "Showing the last known completed snapshot while retained data refresh recovers.",
            )}
          </p>
        </>
      ) : null}

      {isCompletedMode ? (
        <>
          <SurfacePanel
            bodyClassName="process-detail-panel__body"
            className="process-detail-panel"
            description={t(
              "process_detail.final_outcome.description",
              "Terminal state language sourced from the durable release process snapshot.",
            )}
            summary={
              <MetaRow className="process-detail-panel__meta-row">
                <MetaItem
                  label={t(
                    "process_detail.final_outcome.summary.terminal_state",
                    "Terminal state",
                  )}
                >
                  {formatLocalizedProcessFeedStatusLabel(
                    t,
                    process.current_step_status || normalizedStatus,
                  )}
                </MetaItem>
                <MetaItem
                  label={t(
                    "process_detail.final_outcome.summary.builds",
                    "Builds",
                  )}
                >
                  {formatLocalizedProcessFeedBuildCount(
                    t,
                    process.total_build_runs,
                  )}
                </MetaItem>
                <MetaItem
                  label={t(
                    "process_detail.final_outcome.summary.publishes",
                    "Publishes",
                  )}
                >
                  {formatLocalizedProcessFeedPublishCount(
                    t,
                    process.total_publish_runs,
                  )}
                </MetaItem>
              </MetaRow>
            }
            title={t("process_detail.final_outcome.title", "Final Outcome")}
            actions={
              onRequestRerun || isCompletedMode ? (
                <div className="process-detail-toolbar">
                  <Button
                    disabled={
                      completedSnapshot.isLoading ||
                      completedSnapshot.isRefreshing
                    }
                    leadingIcon="refresh"
                    onClick={() => void handleRetryCompletedSnapshot()}
                    size="sm"
                    variant="ghost"
                  >
                    {completedSnapshot.isRefreshing
                      ? t(
                          "process_detail.final_outcome.actions.refreshing",
                          "Refreshing retained data...",
                        )
                      : t(
                          "process_detail.final_outcome.actions.refresh",
                          "Refresh retained data",
                        )}
                  </Button>
                  {onRequestRerun ? (
                    <Button
                      disabled={isRequestingRerun}
                      leadingIcon="refresh"
                      onClick={() => void handleRequestRerun()}
                      size="sm"
                      variant="secondary"
                    >
                      {isRequestingRerun
                        ? t(
                            "process_detail.final_outcome.actions.rerunning",
                            "Rerunning...",
                          )
                        : t(
                            "process_detail.final_outcome.actions.rerun",
                            "Rerun process",
                          )}
                    </Button>
                  ) : null}
                </div>
              ) : null
            }
          >
            <div className="process-detail-panel__step-block">
              <p className="process-detail-panel__step-label">{stepLabel}</p>
              {stepDetail ? (
                <p className="process-detail-panel__step-detail">
                  {stepDetail}
                </p>
              ) : null}
            </div>
          </SurfacePanel>

          <ExecutionReportPanel
            deletedRetention={deletedRetention}
            error={completedSnapshot.error}
            interruptionMessage={reportInterruptionMessage}
            isDeletingRetention={isDeletingRetention}
            isRefreshing={completedSnapshot.isRefreshing}
            onDeleteRetainedMaterial={() => {
              void handleRequestDeleteRetainedMaterial();
            }}
            onOpenPath={(path) => {
              void handleOpenPath(path);
            }}
            onOpenReportViewer={() => {
              void handleOpenReportViewer();
            }}
            onRetry={handleRetryCompletedSnapshot}
            pendingOpenPath={pendingOpenPath}
            report={completedSnapshot.executionReport}
            reportSummaryItems={reportSummaryItems}
            retainedDirPath={retainedDirPath}
            retentionAnchorBuildRunId={retentionAnchorBuildRunId}
            showsLoading={showsCompletedSnapshotLoading}
            showsUnavailable={showsCompletedSnapshotUnavailable}
          />

          {completedSnapshot.isAvailable ? (
            <OutputsPanel
              artifacts={completedSnapshot.artifacts}
              deletedOutputs={deletedOutputs}
              formatArtifactActiveLocationKindLabel={(kind) =>
                formatArtifactActiveLocationKindLabel(t, kind)
              }
              formatArtifactActiveLocationSummary={(artifact) =>
                formatArtifactActiveLocationSummary(t, artifact)
              }
              formatArtifactPublishRunSummary={(publishRun) =>
                formatArtifactPublishRunSummary(t, publishRun)
              }
              formatByteSize={(size) => formatByteSize(t, size)}
              formatPublishTargetKindLabel={(kind) =>
                formatPublishTargetKindLabel(t, kind)
              }
              isDeletingOutputs={isDeletingOutputs}
              onInspectArtifact={(artifact) => {
                void handleOpenArtifactViewer(artifact);
              }}
              onOpenOutputs={(path) => {
                void handleOpenPath(path);
              }}
              onRequestDeleteOutputs={() => {
                void handleRequestDeleteOutputs();
              }}
              outputsPath={completedSnapshot.outputsPath}
              pendingOpenPath={pendingOpenPath}
              resolveArtifactPublishRunTone={resolveArtifactPublishRunTone}
              resolvePublishTargetKindTone={resolvePublishTargetKindTone}
            />
          ) : null}

          {completedSnapshot.isAvailable ? (
            <RetainedLogsPanel
              deletedRetention={deletedRetention}
              entries={retainedLogEntries}
              formatByteSize={(size) => formatByteSize(t, size)}
              logPreviewStatesByEntryPath={logPreviewsByEntryPath}
              logsArchiveExists={Boolean(
                completedSnapshot.executionReport?.logs_archive_exists,
              )}
              logsArchivePath={logsArchivePath}
              onOpenArchive={(path) => {
                void handleOpenPath(path);
              }}
              onOpenViewer={(entryPath, entryName) => {
                void handleOpenRetainedLogViewer(entryPath, entryName);
              }}
              pendingLogPreviewEntryPath={pendingLogPreviewEntryPath}
              pendingOpenPath={pendingOpenPath}
            />
          ) : null}
        </>
      ) : (
        <SurfacePanel
          bodyClassName="process-detail-panel__body"
          className="process-detail-panel"
          description={t(
            "process_detail.current_step.description",
            "Short operator-facing progress language sourced from the runtime process feed.",
          )}
          summary={
            <MetaRow className="process-detail-panel__meta-row">
              <MetaItem
                label={t(
                  "process_detail.current_step.summary.step_state",
                  "Step state",
                )}
              >
                {formatLocalizedProcessFeedStatusLabel(
                  t,
                  process.current_step_status || normalizedStatus,
                )}
              </MetaItem>
              <MetaItem
                label={t(
                  "process_detail.current_step.summary.builds",
                  "Builds",
                )}
              >
                {formatLocalizedProcessFeedBuildCount(
                  t,
                  process.total_build_runs,
                )}
              </MetaItem>
              <MetaItem
                label={t(
                  "process_detail.current_step.summary.publishes",
                  "Publishes",
                )}
              >
                {formatLocalizedProcessFeedPublishCount(
                  t,
                  process.total_publish_runs,
                )}
              </MetaItem>
            </MetaRow>
          }
          actions={
            canRequestCancel ? (
              <div className="process-detail-toolbar">
                <Button
                  disabled={isCancelingProcess || !onRequestCancel}
                  leadingIcon="close"
                  onClick={() => {
                    void handleRequestCancel();
                  }}
                  size="sm"
                  variant="secondary"
                >
                  {isCancelingProcess
                    ? t(
                        "process_detail.current_step.actions.canceling",
                        "Interrupting...",
                      )
                    : t(
                        "process_detail.current_step.actions.cancel",
                        "Interrupt process",
                      )}
                </Button>
              </div>
            ) : null
          }
          title={t("process_detail.current_step.title", "Current Step")}
        >
          <div className="process-detail-panel__step-block">
            <p className="process-detail-panel__step-label">{stepLabel}</p>
            {stepDetail ? (
              <p className="process-detail-panel__step-detail">{stepDetail}</p>
            ) : null}
            {isOnHold ? (
              <p className="process-detail-panel__step-detail">
                {t(
                  "process_detail.current_step.on_hold.guidance",
                  "Close Unity Editor to continue this process. HGP blocks this step intentionally to keep automation consistent, because changing files while a local snapshot is being prepared can invalidate build inputs.",
                )}
              </p>
            ) : null}
          </div>
        </SurfacePanel>
      )}

      <SurfacePanel
        bodyClassName="process-detail-panel__body"
        description={t(
          "process_detail.runtime_metadata.description",
          "Durable runtime timestamps and identifiers currently attached to this release process.",
        )}
        summary={
          <>
            <MetaRow className="process-detail-panel__meta-row">
              <MetaItem
                label={t("process_detail.runtime_metadata.started", "Started")}
              >
                {formatLocalizedProcessFeedMetaValue(
                  t,
                  process.started_at,
                  "process_detail.meta.not_started",
                  "not started",
                )}
              </MetaItem>
              <MetaItem
                label={t(
                  "process_detail.runtime_metadata.finished",
                  "Finished",
                )}
              >
                {formatLocalizedProcessFeedMetaValue(
                  t,
                  process.finished_at,
                  "process_detail.meta.still_active",
                  "still active",
                )}
              </MetaItem>
              <MetaItem
                label={t("process_detail.runtime_metadata.updated", "Updated")}
              >
                {formatLocalizedProcessFeedMetaValue(t, process.updated_at)}
              </MetaItem>
            </MetaRow>

            <MetaRow className="process-detail-panel__meta-row">
              <MetaItem
                label={t("process_detail.runtime_metadata.commit", "Commit")}
              >
                {formatLocalizedProcessFeedMetaValue(t, process.git_commit)}
              </MetaItem>
              <MetaItem
                label={t("process_detail.runtime_metadata.created", "Created")}
              >
                {formatLocalizedProcessFeedMetaValue(t, process.created_at)}
              </MetaItem>
              <MetaItem
                label={t(
                  "process_detail.runtime_metadata.repository",
                  "Repository",
                )}
              >
                {resolveProcessRepositoryLocationLabel(
                  t,
                  process.repository_url,
                )}
              </MetaItem>
            </MetaRow>
          </>
        }
        title={t("process_detail.runtime_metadata.title", "Runtime Metadata")}
        tone="inset"
      >
        <p className="process-detail-report__copy">
          {t(
            "process_detail.runtime_metadata.copy",
            "Runtime identity stays pinned in the shared summary strip so the panel body can stay compact.",
          )}
        </p>
      </SurfacePanel>
    </ScreenScaffold>
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

function isProcessOnHold(process: ProcessFeedRecord) {
  return (
    normalizeProcessFeedDisplayStatus(process.current_step_status) ===
      "on_hold" ||
    process.current_step_status.trim().toLowerCase() === "on_hold"
  );
}

function resolveProcessRepositoryLocationLabel(
  translate: Translate,
  repositoryUrl: string | null,
) {
  const normalizedUrl = repositoryUrl?.trim();
  if (normalizedUrl) {
    return normalizedUrl;
  }

  return translate(
    "process_detail.runtime_metadata.local_workspace",
    "Local workspace",
  );
}

function buildRetainedLogViewerMeta(
  t: Translate,
  payload: RetainedLogArchiveEntryPreviewPayload,
) {
  if (payload.truncated) {
    return t(
      "process_detail.logs.viewer.meta.truncated",
      "Showing the last {{maxBytes}} of {{sizeBytes}} from {{archivePath}}.",
      {
        maxBytes: formatByteSize(t, DEFAULT_LOG_PREVIEW_MAX_BYTES),
        sizeBytes: formatByteSize(t, payload.size_bytes),
        archivePath: payload.archive_path,
      },
    );
  }

  return t(
    "process_detail.logs.viewer.meta.full",
    "Showing the full log file ({{sizeBytes}}) from {{archivePath}}.",
    {
      sizeBytes: formatByteSize(t, payload.size_bytes),
      archivePath: payload.archive_path,
    },
  );
}

function resolveLogViewerFileName(
  value: string | null | undefined,
  fallback: string,
) {
  const normalizedValue = value?.trim();

  if (!normalizedValue) {
    return fallback;
  }

  const pathSeparators = [
    normalizedValue.lastIndexOf("/"),
    normalizedValue.lastIndexOf("\\"),
  ];
  const separatorIndex = Math.max(...pathSeparators);
  const fileName = normalizedValue.slice(separatorIndex + 1).trim();

  return fileName || fallback;
}

function buildOngoingProcessDescription(
  t: Translate,
  usesLiveSnapshot: boolean,
) {
  return usesLiveSnapshot
    ? t(
        "process_detail.frame.ongoing.live",
        "The shell is rendering the latest runtime snapshot for an ongoing process.",
      )
    : t(
        "process_detail.frame.ongoing.cached",
        "The shell is rendering the last cached snapshot for an ongoing process because this release is no longer visible on the current feed page.",
      );
}

function buildCompletedProcessDescription(
  t: Translate,
  usesLiveSnapshot: boolean,
) {
  return usesLiveSnapshot
    ? t(
        "process_detail.frame.completed.live",
        "The shell is rendering the durable report view for a completed process together with its retained outputs and logs.",
      )
    : t(
        "process_detail.frame.completed.cached",
        "The shell is rendering the last cached completed-process snapshot together with durable report data because this release is no longer visible on the current feed page.",
      );
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

function formatArtifactActiveLocationSummary(
  t: Translate,
  artifact: ArtifactInspectionRecord,
) {
  return `${formatArtifactActiveLocationKindLabel(
    t,
    artifact.artifact_active_location_kind,
  )}: ${artifact.artifact_active_location_ref}`;
}

function formatArtifactActiveLocationKindLabel(t: Translate, kind: string) {
  switch (kind.trim().toLocaleLowerCase()) {
    case "runtime_artifact":
      return t(
        "process_detail.outputs.location_kind.runtime_artifact",
        "Managed output root",
      );
    case "filesystem_absolute":
      return t(
        "process_detail.outputs.location_kind.filesystem_absolute",
        "Filesystem publish move",
      );
    default:
      return kind.replace(/_/g, " ");
  }
}

function formatArtifactPublishRunSummary(
  t: Translate,
  publishRun: ArtifactInspectionRecord["publish_runs"][number],
) {
  if (publishRun.destination_ref?.trim()) {
    return publishRun.destination_ref;
  }

  return t(
    "process_detail.outputs.publish_destination_pending",
    "Destination reference pending.",
  );
}

function resolveArtifactPublishRunTone(
  status: string,
): "strong" | "neutral" | "muted" {
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

function resolvePublishTargetKindTone(): "muted" {
  return "muted";
}

function formatPublishTargetKindLabel(t: Translate, kind: string) {
  switch (kind.trim().toLocaleLowerCase()) {
    case "filesystem":
      return t(
        "process_detail.outputs.publish_target_kind.filesystem",
        "Move To Folder",
      );
    case "itch":
      return t(
        "process_detail.outputs.publish_target_kind.itch",
        "Itch.io Upload",
      );
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

function buildExecutionReportSummary(t: Translate, report: JsonValue | null) {
  const items = [
    buildSummaryItem(
      t("process_detail.execution_report.summary.schema", "Schema"),
      readJsonNumber(report, ["schema_version"]),
    ),
    buildSummaryItem(
      t("process_detail.execution_report.summary.build_state", "Build state"),
      readJsonString(report, ["build_run", "status"]),
    ),
    buildSummaryItem(
      t("process_detail.execution_report.summary.cleanup", "Cleanup"),
      readJsonString(report, ["cleanup", "status"]),
    ),
    buildSummaryItem(
      t("process_detail.execution_report.summary.trigger", "Trigger"),
      readJsonString(report, ["cleanup", "trigger"]),
    ),
    buildSummaryItem(
      t("process_detail.execution_report.summary.attempts", "Attempts"),
      readJsonArrayLength(report, ["attempts"]),
    ),
    buildSummaryItem(
      t("process_detail.execution_report.summary.stages", "Stages"),
      readJsonArrayLength(report, ["stages"]),
    ),
  ];

  return items.filter(
    (item): item is { label: string; value: string } => item !== null,
  );
}

function resolveReportInterruptionMessage(
  t: Translate,
  report: JsonValue | null,
) {
  const kind = readJsonString(report, ["interruption", "kind"]);
  const message = readJsonString(report, ["interruption", "message"]);

  if (!kind && !message) {
    return null;
  }

  if (kind && message) {
    return t(
      "process_detail.execution_report.interruption.full",
      "Interruption: {{kind}} · {{message}}",
      { kind, message },
    );
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

function formatByteSize(t: Translate, sizeBytes: number | null) {
  if (sizeBytes === null || Number.isNaN(sizeBytes)) {
    return t("process_detail.byte_size.unknown", "unknown");
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

function buildProcessDetailErrorMessage(t: Translate, error: unknown) {
  if (typeof error === "string" && error.trim()) {
    return error;
  }

  if (error instanceof Error && error.message.trim()) {
    return error.message;
  }

  return t(
    "process_detail.error.fallback",
    "The desktop shell could not load process report data.",
  );
}
