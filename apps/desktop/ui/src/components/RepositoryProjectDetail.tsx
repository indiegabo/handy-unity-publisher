import { startTransition, useEffect, useEffectEvent, useState } from "react";

import { Button } from "./Button";
import { Badge, SurfacePanel } from "./Surface";
import {
  loadRepositoryInspection,
  type RepositoryInspectionEntry,
} from "../services/projects";

type RepositoryProjectDetailProps = {
  repositoryId: number;
};

export function RepositoryProjectDetail({
  repositoryId,
}: RepositoryProjectDetailProps) {
  const [repository, setRepository] =
    useState<RepositoryInspectionEntry | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadRepositoryDetail = useEffectEvent(async () => {
    setIsLoading(true);

    try {
      const inspection = await loadRepositoryInspection();
      const matchingRepository =
        inspection.repositories.find(
          (entry) => entry.repository_id === repositoryId,
        ) ?? null;

      startTransition(() => {
        setRepository(matchingRepository);
        setError(null);
        setIsLoading(false);
      });
    } catch (loadError) {
      startTransition(() => {
        setError(buildProjectDetailErrorMessage(loadError));
        setIsLoading(false);
      });
    }
  });

  useEffect(() => {
    void loadRepositoryDetail();
  }, [repositoryId]);

  if (isLoading) {
    return (
      <div className="project-detail-shell">
        <div className="feed-state">
          <p className="feed-state__title">Loading project detail...</p>
          <p className="feed-state__copy">
            The shell is resolving the repository configuration that was just
            created.
          </p>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="project-detail-shell">
        <p className="feed-banner feed-banner--error">{error}</p>
      </div>
    );
  }

  if (!repository) {
    return (
      <div className="project-detail-shell">
        <div className="feed-state">
          <p className="feed-state__title">Project not found.</p>
          <p className="feed-state__copy">
            The repository was created, but the current inspection payload does
            not include it yet.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="project-detail-shell">
      <SurfacePanel
        actions={
          <Button
            leadingIcon="refresh"
            onClick={() => void loadRepositoryDetail()}
            size="sm"
            variant="secondary"
          >
            Refresh
          </Button>
        }
        description={repository.repo_url}
        eyebrow="Repository Project"
        title={repository.repository_name}
      >
        <div className="project-detail-summary">
          <Badge tone="neutral">
            Poll every {repository.polling_interval_seconds}s
          </Badge>
          <Badge tone={repository.enabled ? "strong" : "muted"}>
            {repository.enabled ? "enabled" : "disabled"}
          </Badge>
          <Badge tone="muted">
            {repository.enabled_build_target_count} active target
            {repository.enabled_build_target_count === 1 ? "" : "s"}
          </Badge>
          <Badge tone="muted">
            {repository.credentials
              ? `credential: ${repository.credentials.name}`
              : "no repository credential"}
          </Badge>
        </div>
      </SurfacePanel>

      <SurfacePanel
        description="Host-native targets registered for this repository."
        eyebrow="Execution"
        title="Build Targets"
      >
        {repository.build_targets.length === 0 ? (
          <div className="feed-state">
            <p className="feed-state__title">No build targets configured.</p>
            <p className="feed-state__copy">
              This repository will not produce build work until at least one
              target is enabled.
            </p>
          </div>
        ) : (
          <div className="project-detail-target-list">
            {repository.build_targets.map((target) => (
              <section
                className="project-detail-target-card"
                key={target.build_target_id}
              >
                <div className="project-detail-target-card__header">
                  <div className="project-detail-target-card__title-block">
                    <h3 className="project-detail-target-card__title">
                      {target.target_name}
                    </h3>
                    <p className="project-detail-target-card__copy">
                      {target.build_method || "Build method not configured"}
                    </p>
                  </div>
                  <div className="project-detail-target-card__badges">
                    <Badge tone="neutral">{target.platform}</Badge>
                    <Badge
                      tone={
                        target.diagnostic_status === "ready"
                          ? "strong"
                          : "muted"
                      }
                    >
                      {target.diagnostic_status.replace(/_/g, " ")}
                    </Badge>
                  </div>
                </div>
                <p className="project-detail-target-card__copy project-detail-target-card__copy--muted">
                  {target.diagnostic_message}
                </p>
              </section>
            ))}
          </div>
        )}
      </SurfacePanel>

      <SurfacePanel
        description="Queue and execution backlog for the registered repository."
        eyebrow="Automation"
        title="Runtime Status"
      >
        <div className="project-detail-status-grid">
          <div className="project-detail-status-card">
            <strong>{repository.pending_release_count}</strong>
            <span>Pending releases</span>
          </div>
          <div className="project-detail-status-card">
            <strong>{repository.queued_build_runs}</strong>
            <span>Queued builds</span>
          </div>
          <div className="project-detail-status-card">
            <strong>{repository.running_build_runs}</strong>
            <span>Running builds</span>
          </div>
          <div className="project-detail-status-card">
            <strong>{repository.running_publish_runs}</strong>
            <span>Running publishes</span>
          </div>
        </div>
      </SurfacePanel>
    </div>
  );
}

function buildProjectDetailErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return "The desktop shell could not load the created project detail.";
}
