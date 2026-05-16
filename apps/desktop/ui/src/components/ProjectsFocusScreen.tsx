import {
  startTransition,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";

import { Button } from "./Button";
import { Icon } from "./Icon";
import { Badge, SurfacePanel } from "./Surface";
import {
  loadRepositoryInspection,
  type RepositoryInspectionEntry,
} from "../services/projects";

type ProjectsFocusScreenProps = {
  highlightedRepositoryId?: number | null;
  onOpenProject: (repositoryId: number) => void;
};

export function ProjectsFocusScreen({
  highlightedRepositoryId = null,
  onOpenProject,
}: ProjectsFocusScreenProps) {
  const [repositories, setRepositories] = useState<RepositoryInspectionEntry[]>(
    [],
  );
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const projectCardRefs = useRef<Record<number, HTMLButtonElement | null>>({});

  const highlightedProject =
    highlightedRepositoryId === null
      ? null
      : repositories.find(
          (repository) => repository.repository_id === highlightedRepositoryId,
        ) ?? null;

  const loadProjects = useEffectEvent(async () => {
    setIsLoading(true);

    try {
      const inspection = await loadRepositoryInspection();

      startTransition(() => {
        setRepositories(inspection.repositories);
        setError(null);
        setIsLoading(false);
      });
    } catch (loadError) {
      startTransition(() => {
        setError(buildProjectsListErrorMessage(loadError));
        setIsLoading(false);
      });
    }
  });

  useEffect(() => {
    void loadProjects();
  }, []);

  useEffect(() => {
    if (isLoading || highlightedRepositoryId === null) {
      return;
    }

    const highlightedCard = projectCardRefs.current[highlightedRepositoryId];
    highlightedCard?.scrollIntoView({
      block: "nearest",
      behavior: "smooth",
    });
  }, [highlightedRepositoryId, isLoading, repositories]);

  return (
    <div className="project-list-shell">
      {error ? <p className="feed-banner feed-banner--error">{error}</p> : null}
      {highlightedProject ? (
        <p className="notice-banner">
          {`${highlightedProject.repository_name} was created. Open it to continue editing.`}
        </p>
      ) : null}

      <SurfacePanel
        actions={
          <Button
            leadingIcon="refresh"
            onClick={() => void loadProjects()}
            size="sm"
            variant="secondary"
          >
            Refresh
          </Button>
        }
        description="Repository projects registered in the local runtime."
        eyebrow="Projects"
        title="Project List"
      >
        {isLoading ? (
          <div className="feed-state">
            <p className="feed-state__title">Loading projects...</p>
            <p className="feed-state__copy">
              The shell is resolving the latest repository inspection snapshot.
            </p>
          </div>
        ) : null}

        {!isLoading && repositories.length === 0 ? (
          <div className="feed-state">
            <p className="feed-state__title">No projects configured yet.</p>
            <p className="feed-state__copy">
              Create a repository project from the home screen to manage it
              here.
            </p>
          </div>
        ) : null}

        {!isLoading && repositories.length > 0 ? (
          <div className="project-list-grid">
            {repositories.map((repository) => (
              <button
                className={joinClassNames(
                  "project-list-card",
                  repository.repository_id === highlightedRepositoryId &&
                    "project-list-card--highlighted",
                )}
                key={repository.repository_id}
                onClick={() => onOpenProject(repository.repository_id)}
                ref={(element) => {
                  projectCardRefs.current[repository.repository_id] = element;
                }}
                type="button"
              >
                <div className="project-list-card__header">
                  <div className="project-list-card__title-block">
                    <h3 className="project-list-card__title">
                      {repository.repository_name}
                    </h3>
                    <p className="project-list-card__copy">
                      {repository.repo_url}
                    </p>
                  </div>

                  <span className="project-list-card__direction">
                    <span className="project-list-card__direction-label">
                      Edit
                    </span>
                    <Icon name="arrowUpRight" size={14} />
                  </span>
                </div>

                <div className="project-list-card__meta">
                  {repository.repository_id === highlightedRepositoryId ? (
                    <Badge tone="strong">new</Badge>
                  ) : null}
                  <Badge tone="neutral">engine: {repository.engine_kind}</Badge>
                  <Badge tone={repository.enabled ? "strong" : "muted"}>
                    {repository.enabled ? "enabled" : "disabled"}
                  </Badge>
                  <Badge tone="neutral">
                    Poll every {repository.polling_interval_seconds}s
                  </Badge>
                  <Badge tone="muted">
                    {repository.enabled_build_target_count} active target
                    {repository.enabled_build_target_count === 1 ? "" : "s"}
                  </Badge>
                </div>

                <p className="project-list-card__copy project-list-card__copy--muted">
                  {buildRepositorySummary(repository)}
                </p>
              </button>
            ))}
          </div>
        ) : null}
      </SurfacePanel>
    </div>
  );
}

function buildRepositorySummary(repository: RepositoryInspectionEntry) {
  const lastSeenTag = repository.last_seen_tag
    ? `Last seen tag ${repository.last_seen_tag}.`
    : "No baseline tag recorded yet.";

  const publishTargetCount = repository.publish_targets.length;

  return `${lastSeenTag} ${publishTargetCount} publish target${publishTargetCount === 1 ? "" : "s"} registered.`;
}

function buildProjectsListErrorMessage(error: unknown) {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return "The desktop shell could not load the project list.";
}

function joinClassNames(...tokens: Array<string | false | null | undefined>) {
  return tokens.filter(Boolean).join(" ");
}
