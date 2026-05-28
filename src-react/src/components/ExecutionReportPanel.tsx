import { Button } from "./Button";
import { useLocalization } from "../LocalizationProvider";
import { MetaItem, MetaRow, SurfacePanel } from "./Surface";

type ExecutionReportPanelProps = {
  deletedRetention: boolean;
  error: string | null;
  interruptionMessage: string | null;
  isDeletingRetention: boolean;
  isRefreshing: boolean;
  onDeleteRetainedMaterial: () => void;
  onOpenPath: (path: string) => void;
  onOpenReportViewer: () => void;
  onRetry: () => void;
  pendingOpenPath: string | null;
  report: {
    exists: boolean;
    report: unknown | null;
    report_path: string | null;
  } | null;
  reportSummaryItems: ReadonlyArray<{
    label: string;
    value: string;
  }>;
  retainedDirPath: string | null;
  retentionAnchorBuildRunId: number | null;
  showsLoading: boolean;
  showsUnavailable: boolean;
};

export function ExecutionReportPanel({
  deletedRetention,
  error,
  interruptionMessage,
  isDeletingRetention,
  isRefreshing,
  onDeleteRetainedMaterial,
  onOpenPath,
  onOpenReportViewer,
  onRetry,
  pendingOpenPath,
  report,
  reportSummaryItems,
  retainedDirPath,
  retentionAnchorBuildRunId,
  showsLoading,
  showsUnavailable,
}: ExecutionReportPanelProps) {
  const { t } = useLocalization();
  const reportPath = report?.report_path ?? null;

  return (
    <SurfacePanel
      bodyClassName="process-detail-panel__body"
      className="process-detail-report-panel"
      description={t(
        "process_detail.execution_report.description",
        "Retained report data captured after this release process reached a terminal state.",
      )}
      summary={
        report?.exists && report.report && reportSummaryItems.length > 0 ? (
          <MetaRow className="process-detail-panel__meta-row">
            {reportSummaryItems.map((item) => (
              <MetaItem key={item.label} label={item.label}>
                {item.value}
              </MetaItem>
            ))}
          </MetaRow>
        ) : null
      }
      title={t("process_detail.execution_report.title", "Execution Report")}
      tone="inset"
      actions={
        <div className="process-detail-toolbar process-detail-toolbar--report-header">
          {reportPath ? (
            <Button
              disabled={deletedRetention || pendingOpenPath === reportPath}
              leadingIcon="arrowUpRight"
              onClick={() => onOpenPath(reportPath)}
              size="sm"
              variant="ghost"
            >
              {t(
                "process_detail.execution_report.actions.open_file",
                "Open report file",
              )}
            </Button>
          ) : null}
          {retainedDirPath ? (
            <Button
              disabled={deletedRetention || pendingOpenPath === retainedDirPath}
              leadingIcon="folder"
              onClick={() => onOpenPath(retainedDirPath)}
              size="sm"
              variant="ghost"
            >
              {t(
                "process_detail.execution_report.actions.open_folder",
                "Open retained folder",
              )}
            </Button>
          ) : null}
          {retentionAnchorBuildRunId ? (
            <Button
              disabled={deletedRetention || isDeletingRetention}
              leadingIcon="trash"
              onClick={onDeleteRetainedMaterial}
              size="sm"
              variant="ghost"
            >
              {isDeletingRetention
                ? t(
                    "process_detail.execution_report.actions.deleting",
                    "Deleting...",
                  )
                : t(
                    "process_detail.execution_report.actions.delete",
                    "Delete retained material",
                  )}
            </Button>
          ) : null}
        </div>
      }
    >
      {showsLoading ? (
        <p className="process-detail-report__copy">
          {t(
            "process_detail.execution_report.loading",
            "Loading retained report data for this completed process...",
          )}
        </p>
      ) : showsUnavailable ? (
        <div className="process-detail-toolbar">
          <p className="process-detail-report__copy">{error}</p>
          <Button
            disabled={isRefreshing}
            leadingIcon="refresh"
            onClick={onRetry}
            size="sm"
            variant="secondary"
          >
            {t(
              "process_detail.execution_report.actions.retry",
              "Retry retained data",
            )}
          </Button>
        </div>
      ) : deletedRetention ? (
        <p className="process-detail-report__copy">
          {t(
            "process_detail.execution_report.deleted_copy",
            "The retained report directory for this completed process has been removed from disk.",
          )}
        </p>
      ) : report?.exists && report.report ? (
        <div className="process-detail-report-shell">
          {interruptionMessage ? (
            <p className="process-detail-report__copy">{interruptionMessage}</p>
          ) : null}

          <div className="process-detail-log-preview">
            <p className="process-detail-report__copy">
              {t(
                "process_detail.execution_report.viewer_copy",
                "Open the full retained JSON in the viewer when you need raw details; the inline screen stays compact.",
              )}
            </p>
            <div className="process-detail-toolbar">
              <Button
                leadingIcon="terminal"
                onClick={onOpenReportViewer}
                size="sm"
                variant="ghost"
              >
                {t(
                  "process_detail.execution_report.actions.view_json",
                  "View JSON report",
                )}
              </Button>
            </div>
          </div>
        </div>
      ) : (
        <p className="process-detail-report__copy">
          {t(
            "process_detail.execution_report.empty",
            "No retained execution report file was found for this completed process.",
          )}
        </p>
      )}
    </SurfacePanel>
  );
}
