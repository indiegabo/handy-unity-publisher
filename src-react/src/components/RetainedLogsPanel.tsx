import { Button } from "./Button";
import { useLocalization } from "../LocalizationProvider";
import { Badge, MetaItem, MetaRow, SurfacePanel } from "./Surface";
import { VerticalAccordion } from "./VerticalAccordion";

type RetainedLogEntry = {
  compressed_size_bytes: number;
  entry_name: string;
  entry_path: string;
  size_bytes: number;
};

type RetainedLogPreviewState = {
  error: string | null;
};

type RetainedLogsPanelProps = {
  deletedRetention: boolean;
  entries: readonly RetainedLogEntry[];
  formatByteSize: (size: number) => string;
  logPreviewStatesByEntryPath: Record<
    string,
    RetainedLogPreviewState | undefined
  >;
  logsArchiveExists: boolean;
  logsArchivePath: string | null;
  onOpenArchive: (path: string) => void;
  onOpenViewer: (entryPath: string, entryName: string) => void;
  pendingLogPreviewEntryPath: string | null;
  pendingOpenPath: string | null;
};

export function RetainedLogsPanel({
  deletedRetention,
  entries,
  formatByteSize,
  logPreviewStatesByEntryPath,
  logsArchiveExists,
  logsArchivePath,
  onOpenArchive,
  onOpenViewer,
  pendingLogPreviewEntryPath,
  pendingOpenPath,
}: RetainedLogsPanelProps) {
  const { t } = useLocalization();

  return (
    <SurfacePanel
      bodyClassName="process-detail-panel__body"
      description={t(
        "process_detail.retained_logs.description",
        "Archived execution log entries stored under retained/execution-logs.zip for this completed process.",
      )}
      summary={
        <MetaRow className="process-detail-panel__meta-row">
          <MetaItem
            label={t(
              "process_detail.retained_logs.summary.archive_path",
              "Archive path",
            )}
          >
            {logsArchivePath ||
              t("process_detail.meta.not_recorded", "not recorded")}
          </MetaItem>
          <MetaItem
            label={t("process_detail.retained_logs.summary.entries", "Entries")}
          >
            {String(entries.length)}
          </MetaItem>
        </MetaRow>
      }
      title={t("process_detail.retained_logs.title", "Retained Logs")}
      actions={
        logsArchivePath ? (
          <Button
            disabled={deletedRetention || pendingOpenPath === logsArchivePath}
            leadingIcon="arrowUpRight"
            onClick={() => onOpenArchive(logsArchivePath)}
            size="sm"
            variant="ghost"
          >
            {t(
              "process_detail.retained_logs.actions.open_archive",
              "Open log archive",
            )}
          </Button>
        ) : null
      }
    >
      {deletedRetention ? (
        <p className="process-detail-report__copy">
          {t(
            "process_detail.retained_logs.deleted_copy",
            "The retained log archive for this completed process has been removed from disk.",
          )}
        </p>
      ) : !logsArchiveExists ? (
        <p className="process-detail-report__copy">
          {t(
            "process_detail.retained_logs.empty_missing",
            "No retained log archive was found for this completed process.",
          )}
        </p>
      ) : entries.length === 0 ? (
        <p className="process-detail-report__copy">
          {t(
            "process_detail.retained_logs.empty_no_entries",
            "The retained log archive exists, but it does not contain any readable log entries.",
          )}
        </p>
      ) : (
        <div className="process-detail-log-list">
          {entries.map((entry) => {
            const logPreview = logPreviewStatesByEntryPath[entry.entry_path];

            return (
              <VerticalAccordion
                bodyInset
                className="process-detail-log-card"
                collapsedToggleLabel={t(
                  "process_detail.retained_logs.accordion.expand",
                  "Expand retained log {{entryName}}",
                  { entryName: entry.entry_name },
                )}
                expandedToggleLabel={t(
                  "process_detail.retained_logs.accordion.collapse",
                  "Collapse retained log {{entryName}}",
                  { entryName: entry.entry_name },
                )}
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
                        {t(
                          "process_detail.retained_logs.zipped",
                          "{{size}} zipped",
                          {
                            size: formatByteSize(entry.compressed_size_bytes),
                          },
                        )}
                      </Badge>
                    </div>
                  </div>
                }
                headerSeparated
                key={entry.entry_path}
                tone="section"
              >
                <div className="process-detail-log-card__body">
                  <div className="process-detail-toolbar">
                    <Button
                      aria-label={t(
                        "process_detail.retained_logs.actions.open_viewer_named",
                        "Open retained log viewer for {{entryName}}",
                        { entryName: entry.entry_name },
                      )}
                      disabled={
                        deletedRetention ||
                        pendingLogPreviewEntryPath === entry.entry_path
                      }
                      leadingIcon="terminal"
                      onClick={() =>
                        onOpenViewer(entry.entry_path, entry.entry_name)
                      }
                      size="sm"
                      title={t(
                        "process_detail.retained_logs.actions.open_viewer_named",
                        "Open retained log viewer for {{entryName}}",
                        { entryName: entry.entry_name },
                      )}
                      variant="ghost"
                    >
                      {pendingLogPreviewEntryPath === entry.entry_path
                        ? t(
                            "process_detail.retained_logs.actions.loading_viewer",
                            "Loading viewer...",
                          )
                        : t(
                            "process_detail.retained_logs.actions.open_viewer",
                            "Open viewer",
                          )}
                    </Button>
                  </div>

                  <MetaRow className="process-detail-panel__meta-row">
                    <MetaItem
                      label={t(
                        "process_detail.retained_logs.summary.archive_path",
                        "Archive path",
                      )}
                    >
                      {logsArchivePath ||
                        t("process_detail.meta.not_recorded", "not recorded")}
                    </MetaItem>
                    <MetaItem
                      label={t(
                        "process_detail.retained_logs.labels.expanded_size",
                        "Expanded size",
                      )}
                    >
                      {formatByteSize(entry.size_bytes)}
                    </MetaItem>
                    <MetaItem
                      label={t(
                        "process_detail.retained_logs.labels.compressed_size",
                        "Compressed size",
                      )}
                    >
                      {formatByteSize(entry.compressed_size_bytes)}
                    </MetaItem>
                  </MetaRow>

                  {logPreview?.error ? (
                    <p className="process-detail-report__copy">
                      {logPreview.error}
                    </p>
                  ) : null}

                  <p className="process-detail-report__copy">
                    {t(
                      "process_detail.retained_logs.viewer_copy",
                      "Open the retained log viewer to inspect this entry without expanding the full log body inline.",
                    )}
                  </p>
                </div>
              </VerticalAccordion>
            );
          })}
        </div>
      )}
    </SurfacePanel>
  );
}
