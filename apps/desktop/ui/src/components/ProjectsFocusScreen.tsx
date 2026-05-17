import {
  startTransition,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";

import { Button } from "./Button";
import { Icon } from "./Icon";
import {
  Badge,
  FocusPageFrame,
  MetaItem,
  MetaRow,
  SurfacePanel,
} from "./Surface";
import {
  loadRepositoryInspection,
  type RepositoryInspectionEntry,
} from "../services/projects";

type ProjectsFocusScreenProps = {
  highlightedRepositoryId?: number | null;
  onOpenProject: (repositoryId: number, repositoryName: string) => void;
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
      : (repositories.find(
          (repository) => repository.repository_id === highlightedRepositoryId,
        ) ?? null);
  const enabledRepositoryCount = repositories.filter(
    (repository) => repository.enabled,
  ).length;
  const disabledRepositoryCount = repositories.length - enabledRepositoryCount;
  const activeBuildTargetCount = repositories.reduce(
    (total, repository) => total + repository.enabled_build_target_count,
    0,
  );

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
      <FocusPageFrame
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
        description="Review repository projects registered in the local runtime and open one to inspect or edit its pipeline settings."
        eyebrow="Projects"
        summary={
          <MetaRow>
            <MetaItem label="Projects">
              {isLoading
                ? "Loading snapshot..."
                : `${repositories.length} registered`}
            </MetaItem>
            {!isLoading ? (
              <MetaItem label="Enabled">{enabledRepositoryCount}</MetaItem>
            ) : null}
            {!isLoading ? (
              <MetaItem label="Disabled">{disabledRepositoryCount}</MetaItem>
            ) : null}
            {!isLoading ? (
              <MetaItem label="Active targets">
                {activeBuildTargetCount}
              </MetaItem>
            ) : null}
          </MetaRow>
        }
        title="Project List"
      >
        {error ? (
          <p className="feed-banner feed-banner--error">{error}</p>
        ) : null}
        {highlightedProject ? (
          <p className="notice-banner">
            {`${highlightedProject.repository_name} was created. Open it to continue editing.`}
          </p>
        ) : null}

        <SurfacePanel
          className="project-list-section"
          description="Repository projects currently known by the local runtime."
          eyebrow="Registered Projects"
          headerSeparated
          title="Project Inventory"
          tone="section"
        >
          {isLoading ? (
            <div className="feed-state">
              <p className="feed-state__title">Loading projects...</p>
              <p className="feed-state__copy">
                The shell is resolving the latest repository inspection
                snapshot.
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
                  onClick={() =>
                    onOpenProject(
                      repository.repository_id,
                      repository.repository_name,
                    )
                  }
                  ref={(element) => {
                    projectCardRefs.current[repository.repository_id] = element;
                  }}
                  type="button"
                >
                  <div className="project-list-card__header">
                    <div className="project-list-card__title-block">
                      <div className="project-list-card__title-row">
                        <h3 className="project-list-card__title">
                          {repository.repository_name}
                        </h3>
                        <div className="project-list-card__badges">
                          {repository.repository_id ===
                          highlightedRepositoryId ? (
                            <Badge tone="strong">new</Badge>
                          ) : null}
                          <Badge tone={repository.enabled ? "strong" : "muted"}>
                            {repository.enabled ? "enabled" : "disabled"}
                          </Badge>
                        </div>
                      </div>
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

                  <MetaRow className="project-list-card__meta">
                    <MetaItem label="Engine">{repository.engine_kind}</MetaItem>
                    <MetaItem label="Poll">
                      {`${repository.polling_interval_seconds}s cadence`}
                    </MetaItem>
                    <MetaItem label="Targets">
                      {formatTargetCount(repository.enabled_build_target_count)}
                    </MetaItem>
                  </MetaRow>

                  <p className="project-list-card__summary">
                    {buildRepositorySummary(repository)}
                  </p>
                </button>
              ))}
            </div>
          ) : null}
        </SurfacePanel>
      </FocusPageFrame>
    </div>
  );
}

function buildRepositorySummary(repository: RepositoryInspectionEntry) {
  const lastSeenTag = repository.last_seen_tag
    ? `Last seen tag ${repository.last_seen_tag}.`
    : "No baseline tag recorded yet.";

  const publishDestinationCount = repository.publish_targets.length;

  return `${lastSeenTag} ${publishDestinationCount} publish destination${publishDestinationCount === 1 ? "" : "s"} registered.`;
}

function formatTargetCount(targetCount: number) {
  return `${targetCount} active target${targetCount === 1 ? "" : "s"}`;
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
