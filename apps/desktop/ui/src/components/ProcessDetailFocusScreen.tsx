import {
  Badge,
  FocusPageFrame,
  MetaItem,
  MetaRow,
  SurfacePanel,
} from "./Surface";
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

type ProcessDetailFocusScreenProps = {
  process: ProcessFeedRecord | null;
  usesLiveSnapshot: boolean;
};

export function ProcessDetailFocusScreen({
  process,
  usesLiveSnapshot,
}: ProcessDetailFocusScreenProps) {
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

  const normalizedStatus = normalizeProcessFeedDisplayStatus(process.display_status);
  const stepLabel = resolveProcessFeedStepLabel(process, normalizedStatus);
  const stepDetail = resolveProcessFeedStepDetail(process);

  return (
    <FocusPageFrame
      className="process-detail-screen"
      description={
        usesLiveSnapshot
          ? "The shell is rendering the latest runtime snapshot currently visible in the feed."
          : "The shell is rendering the last cached snapshot because this process is no longer on the current feed page."
      }
      eyebrow="Process"
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
        <p className="feed-banner feed-banner--error">{process.error_message}</p>
      ) : null}

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