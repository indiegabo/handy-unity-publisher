import { Button } from "./Button";
import FullScreenModal from "./FullScreenModal";
import {
  Badge,
  type BadgeTone,
  MetaItem,
  MetaRow,
  SummaryStrip,
  SurfacePanel,
} from "./Surface";
import { useLocalization, type Translate } from "../LocalizationProvider";
import type { ArtifactInspectionRecord } from "../services/processDetail";

type ArtifactViewerProps = {
  artifact: ArtifactInspectionRecord;
  artifactAbsolutePath: string | null;
  artifactFolderPath: string | null;
  artifactLocationSummary: string;
  onOpenArtifact?: () => void;
  onOpenFolder?: () => void;
  onResolve?: (value?: null) => void;
  openArtifactDisabled?: boolean;
  openArtifactLabel?: string;
  openFolderDisabled?: boolean;
  openFolderLabel?: string;
  resolvePublishTargetKindTone: (kind: string) => BadgeTone;
};

export function ArtifactViewer({
  artifact,
  artifactAbsolutePath,
  artifactFolderPath,
  artifactLocationSummary,
  onOpenArtifact,
  onOpenFolder,
  onResolve,
  openArtifactDisabled = false,
  openArtifactLabel,
  openFolderDisabled = false,
  openFolderLabel,
  resolvePublishTargetKindTone,
}: ArtifactViewerProps) {
  const { t } = useLocalization();
  const canOpenArtifact =
    !openArtifactDisabled && Boolean(artifactAbsolutePath);
  const canOpenFolder = !openFolderDisabled && Boolean(artifactFolderPath);
  const resolvedOpenArtifactLabel =
    openArtifactLabel ??
    t("artifact_viewer.actions.open_artifact", "Open artifact");
  const resolvedOpenFolderLabel =
    openFolderLabel ?? t("artifact_viewer.actions.open_folder", "Open folder");
  const unavailableLabel = t(
    "artifact_viewer.paths.unavailable",
    "not available",
  );

  return (
    <FullScreenModal
      className="artifact-viewer__modal"
      description={t(
        "artifact_viewer.description",
        "Inspect artifact metadata and host-local paths without bloating the process detail surface.",
      )}
      onResolve={onResolve}
      title={artifact.artifact_name}
    >
      <div className="artifact-viewer">
        <div className="artifact-viewer__summary">
          <p className="artifact-viewer__path">{artifact.artifact_path}</p>

          <SummaryStrip className="artifact-viewer__summary-strip">
            <div className="artifact-viewer__badges">
              <Badge tone="muted">{artifact.artifact_kind}</Badge>
              <Badge tone="neutral">{artifact.build_target_name}</Badge>
              <Badge
                tone={resolveArtifactPublishCountTone(
                  artifact.publish_run_count,
                )}
              >
                {formatPublishCount(t, artifact.publish_run_count)}
              </Badge>
            </div>

            <MetaRow>
              <MetaItem
                label={t("artifact_viewer.summary.active_location", "Active location")}
              >
                {artifactLocationSummary}
              </MetaItem>
              <MetaItem label={t("artifact_viewer.summary.size", "Size")}>
                {formatByteSize(t, artifact.size_bytes)}
              </MetaItem>
            </MetaRow>
          </SummaryStrip>
        </div>

        <SurfacePanel
          actions={
            <div className="artifact-viewer__actions">
              <Button
                data-overlay-autofocus={canOpenArtifact}
                disabled={!canOpenArtifact}
                leadingIcon="arrowUpRight"
                onClick={onOpenArtifact}
                size="sm"
                variant="ghost"
              >
                {resolvedOpenArtifactLabel}
              </Button>
              <Button
                data-overlay-autofocus={!canOpenArtifact && canOpenFolder}
                disabled={!canOpenFolder}
                leadingIcon="folder"
                onClick={onOpenFolder}
                size="sm"
                variant="ghost"
              >
                {resolvedOpenFolderLabel}
              </Button>
            </div>
          }
          description={t(
            "artifact_viewer.paths.description",
            "Host-local paths resolved for this artifact from the current runtime snapshot.",
          )}
          title={t("artifact_viewer.paths.title", "Artifact Paths")}
          tone="inset"
        >
          <MetaRow>
            <MetaItem
              label={t("artifact_viewer.paths.absolute", "Absolute path")}
            >
              {artifactAbsolutePath || unavailableLabel}
            </MetaItem>
            <MetaItem label={t("artifact_viewer.paths.folder", "Folder path")}>
              {artifactFolderPath || unavailableLabel}
            </MetaItem>
          </MetaRow>
          <p className="artifact-viewer__copy">
            {t(
              "artifact_viewer.paths.preview_unavailable",
              "In-shell preview is not available for host-native artifact payloads yet. Use the host actions above to inspect the artifact directly.",
            )}
          </p>
        </SurfacePanel>

        <SurfacePanel
          description={t(
            "artifact_viewer.publish_history.description",
            "Publication records currently attached to this artifact.",
          )}
          title={t("artifact_viewer.publish_history.title", "Publish History")}
          tone="inset"
        >
          {artifact.publish_runs.length === 0 ? (
            <p className="artifact-viewer__copy">
              {t(
                "artifact_viewer.publish_history.empty",
                "No publish runs are currently attached to this artifact.",
              )}
            </p>
          ) : (
            <div className="artifact-viewer__publish-list">
              {artifact.publish_runs.map((publishRun) => (
                <div
                  className="artifact-viewer__publish-run"
                  key={publishRun.publish_run_id}
                >
                  <p className="artifact-viewer__publish-title">
                    {publishRun.publish_target_name}
                  </p>
                  <p className="artifact-viewer__copy">
                    {publishRun.destination_ref ||
                      t(
                        "artifact_viewer.publish_history.pending_destination",
                        "Destination reference pending.",
                      )}
                  </p>
                  <div className="artifact-viewer__badges">
                    <Badge tone={resolvePublishRunTone(publishRun.status)}>
                      {publishRun.status}
                    </Badge>
                    <Badge
                      tone={resolvePublishTargetKindTone(
                        publishRun.publish_target_kind,
                      )}
                    >
                      {formatPublishTargetKindLabel(t, publishRun.publish_target_kind)}
                    </Badge>
                  </div>
                </div>
              ))}
            </div>
          )}
        </SurfacePanel>
      </div>
    </FullScreenModal>
  );
}

function formatPublishCount(t: Translate, count: number) {
  return count === 1
    ? t("artifact_viewer.publish_count.one", "1 publish")
    : t("artifact_viewer.publish_count.other", "{{count}} publishes", {
        count,
      });
}

function resolveArtifactPublishCountTone(count: number): BadgeTone {
  return count > 0 ? "neutral" : "muted";
}

function resolvePublishRunTone(status: string): "strong" | "neutral" | "muted" {
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

function formatPublishTargetKindLabel(t: Translate, kind: string) {
  switch (kind.trim().toLocaleLowerCase()) {
    case "filesystem":
      return t(
        "artifact_viewer.publish_target_kind.filesystem",
        "Move To Folder",
      );
    case "itch":
      return t(
        "artifact_viewer.publish_target_kind.itch",
        "Itch.io Upload",
      );
    default:
      return kind;
  }
}

function formatByteSize(t: Translate, sizeBytes: number | null) {
  if (sizeBytes === null || sizeBytes < 0) {
    return t("artifact_viewer.size.unknown", "unknown");
  }

  if (sizeBytes < 1024) {
    return `${sizeBytes} B`;
  }

  if (sizeBytes < 1024 * 1024) {
    return `${(sizeBytes / 1024).toFixed(1)} KB`;
  }

  if (sizeBytes < 1024 * 1024 * 1024) {
    return `${(sizeBytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  return `${(sizeBytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}
