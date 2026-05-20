import type { ArtifactInspectionRecord } from "../services/processDetail";

import { Button } from "./Button";
import {
  Badge,
  type BadgeTone,
  MetaItem,
  MetaRow,
  SurfacePanel,
} from "./Surface";

type OutputsPanelProps = {
  artifacts: readonly ArtifactInspectionRecord[];
  deletedOutputs: boolean;
  formatArtifactActiveLocationKindLabel: (kind: string) => string;
  formatArtifactActiveLocationSummary: (
    artifact: ArtifactInspectionRecord,
  ) => string;
  formatArtifactPublishRunSummary: (
    publishRun: ArtifactInspectionRecord["publish_runs"][number],
  ) => string;
  formatByteSize: (size: number | null) => string;
  formatPublishTargetKindLabel: (kind: string) => string;
  isDeletingOutputs: boolean;
  onInspectArtifact: (artifact: ArtifactInspectionRecord) => void;
  onOpenOutputs: (path: string) => void;
  onRequestDeleteOutputs: () => void;
  outputsPath: string | null;
  pendingOpenPath: string | null;
  resolveArtifactPublishRunTone: (status: string) => BadgeTone;
  resolvePublishTargetKindTone: (kind: string) => BadgeTone;
};

export function OutputsPanel({
  artifacts,
  deletedOutputs,
  formatArtifactActiveLocationKindLabel,
  formatArtifactActiveLocationSummary,
  formatArtifactPublishRunSummary,
  formatByteSize,
  formatPublishTargetKindLabel,
  isDeletingOutputs,
  onInspectArtifact,
  onOpenOutputs,
  onRequestDeleteOutputs,
  outputsPath,
  pendingOpenPath,
  resolveArtifactPublishRunTone,
  resolvePublishTargetKindTone,
}: OutputsPanelProps) {
  return (
    <SurfacePanel
      bodyClassName="process-detail-panel__body"
      description="Artifacts registered for this process and the shared outputs directory they live under."
      summary={
        <MetaRow className="process-detail-panel__meta-row">
          <MetaItem label="Outputs root">
            {outputsPath || "not recorded"}
          </MetaItem>
          <MetaItem label="Artifacts recorded">
            {String(artifacts.length)}
          </MetaItem>
        </MetaRow>
      }
      title="Outputs"
      actions={
        <div className="process-detail-toolbar">
          {outputsPath ? (
            <Button
              disabled={deletedOutputs || pendingOpenPath === outputsPath}
              leadingIcon="folder"
              onClick={() => onOpenOutputs(outputsPath)}
              size="sm"
              variant="ghost"
            >
              Open outputs
            </Button>
          ) : null}
          {outputsPath ? (
            <Button
              disabled={deletedOutputs || isDeletingOutputs}
              leadingIcon="trash"
              onClick={onRequestDeleteOutputs}
              size="sm"
              variant="ghost"
            >
              {isDeletingOutputs ? "Deleting..." : "Delete outputs"}
            </Button>
          ) : null}
        </div>
      }
    >
      {deletedOutputs ? (
        <p className="process-detail-report__copy">
          The shared outputs directory for this process has been removed from
          disk.
        </p>
      ) : null}

      {artifacts.length === 0 ? (
        <p className="process-detail-report__copy">
          No artifact records are currently attached to this process.
        </p>
      ) : (
        <div className="process-detail-artifact-list">
          {artifacts.map((artifact) => (
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
                    leadingIcon="search"
                    onClick={() => onInspectArtifact(artifact)}
                    size="sm"
                    variant="ghost"
                  >
                    Inspect artifact
                  </Button>
                </div>
              </div>

              <MetaRow className="process-detail-panel__meta-row">
                <MetaItem label="Kind">{artifact.artifact_kind}</MetaItem>
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
                            {formatArtifactPublishRunSummary(publishRun)}
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
                          <Badge
                            tone={resolvePublishTargetKindTone(
                              publishRun.publish_target_kind,
                            )}
                          >
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
          ))}
        </div>
      )}
    </SurfacePanel>
  );
}
