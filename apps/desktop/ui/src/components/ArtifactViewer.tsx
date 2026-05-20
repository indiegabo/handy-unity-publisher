import { Button } from "./Button";
import FullScreenModal from "./FullScreenModal";
import { Badge, MetaItem, MetaRow, SurfacePanel } from "./Surface";
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
  openArtifactLabel = "Open artifact",
  openFolderDisabled = false,
  openFolderLabel = "Open folder",
}: ArtifactViewerProps) {
  const canOpenArtifact =
    !openArtifactDisabled && Boolean(artifactAbsolutePath);
  const canOpenFolder = !openFolderDisabled && Boolean(artifactFolderPath);

  return (
    <FullScreenModal
      className="artifact-viewer__modal"
      description="Inspect artifact metadata and host-local paths without bloating the process detail surface."
      onResolve={onResolve}
      title={artifact.artifact_name}
    >
      <div className="artifact-viewer">
        <div className="artifact-viewer__summary">
          <p className="artifact-viewer__path">{artifact.artifact_path}</p>

          <div className="artifact-viewer__badges">
            <Badge tone="muted">{artifact.artifact_kind}</Badge>
            <Badge tone="muted">{artifact.build_target_name}</Badge>
            <Badge tone="muted">
              {formatPublishCount(artifact.publish_run_count)}
            </Badge>
          </div>

          <MetaRow>
            <MetaItem label="Active location">
              {artifactLocationSummary}
            </MetaItem>
            <MetaItem label="Size">
              {formatByteSize(artifact.size_bytes)}
            </MetaItem>
          </MetaRow>
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
                {openArtifactLabel}
              </Button>
              <Button
                data-overlay-autofocus={!canOpenArtifact && canOpenFolder}
                disabled={!canOpenFolder}
                leadingIcon="folder"
                onClick={onOpenFolder}
                size="sm"
                variant="ghost"
              >
                {openFolderLabel}
              </Button>
            </div>
          }
          description="Host-local paths resolved for this artifact from the current runtime snapshot."
          title="Artifact Paths"
          tone="inset"
        >
          <MetaRow>
            <MetaItem label="Absolute path">
              {artifactAbsolutePath || "not available"}
            </MetaItem>
            <MetaItem label="Folder path">
              {artifactFolderPath || "not available"}
            </MetaItem>
          </MetaRow>
          <p className="artifact-viewer__copy">
            In-shell preview is not available for host-native artifact payloads
            yet. Use the host actions above to inspect the artifact directly.
          </p>
        </SurfacePanel>

        <SurfacePanel
          description="Publication records currently attached to this artifact."
          title="Publish History"
          tone="inset"
        >
          {artifact.publish_runs.length === 0 ? (
            <p className="artifact-viewer__copy">
              No publish runs are currently attached to this artifact.
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
                      "Destination reference pending."}
                  </p>
                  <div className="artifact-viewer__badges">
                    <Badge tone={resolvePublishRunTone(publishRun.status)}>
                      {publishRun.status}
                    </Badge>
                    <Badge tone="neutral">
                      {formatPublishTargetKindLabel(
                        publishRun.publish_target_kind,
                      )}
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

function formatPublishCount(count: number) {
  return `${count} publish${count === 1 ? "" : "es"}`;
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

function formatByteSize(sizeBytes: number | null) {
  if (sizeBytes === null || sizeBytes < 0) {
    return "unknown";
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
