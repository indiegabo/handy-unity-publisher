import {
  startTransition,
  useDeferredValue,
  useEffect,
  useEffectEvent,
  useMemo,
  useRef,
  useState,
} from "react";

import { Button } from "./Button";
import { MetaItem, MetaRow, SurfacePanel } from "./Surface";
import ScreenScaffold from "./ScreenScaffold";
import InputWithPicker from "./InputWithPicker";
import SelectListFullScreen from "./SelectListFullScreen";
import ProjectList from "./projects/ProjectList";
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
  const [quickOpenQuery, setQuickOpenQuery] = useState("");
  const projectCardRefs = useRef<Record<number, HTMLButtonElement | null>>({});
  const deferredQuickOpenQuery = useDeferredValue(quickOpenQuery);

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
  const filteredRepositories = useMemo(
    () => filterRepositories(repositories, deferredQuickOpenQuery),
    [deferredQuickOpenQuery, repositories],
  );
  const quickOpenItems = useMemo(
    () => repositories.map(buildProjectPickerItem),
    [repositories],
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

  const handleQuickOpenPick = useEffectEvent((value: string) => {
    const repository = repositories.find(
      (entry) => String(entry.repository_id) === value,
    );

    if (!repository) {
      return;
    }

    onOpenProject(repository.repository_id, repository.repository_name);
  });

  const handleQuickOpenKeyDown = useEffectEvent(
    (event: React.KeyboardEvent<HTMLInputElement>) => {
      if (event.key === "ArrowDown" && filteredRepositories.length > 0) {
        event.preventDefault();
        projectCardRefs.current[filteredRepositories[0].repository_id]?.focus();
        return;
      }

      if (event.key !== "Enter") {
        return;
      }

      const normalizedQuery = quickOpenQuery.trim().toLowerCase();

      if (!normalizedQuery || filteredRepositories.length === 0) {
        return;
      }

      const exactRepository = filteredRepositories.find((repository) => {
        return (
          repository.repository_name.toLowerCase() === normalizedQuery ||
          repository.repo_url.toLowerCase() === normalizedQuery
        );
      });

      const targetRepository =
        exactRepository ??
        (filteredRepositories.length === 1 ? filteredRepositories[0] : null);

      if (!targetRepository) {
        return;
      }

      event.preventDefault();
      onOpenProject(
        targetRepository.repository_id,
        targetRepository.repository_name,
      );
    },
  );

  return (
    <div className="project-list-shell">
      <ScreenScaffold
        title="Project List"
        subtitle="Review repository projects registered in the local runtime and open one to inspect or edit its pipeline settings."
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
      >
        {error ? (
          <p className="feed-banner feed-banner--error">{error}</p>
        ) : null}
        {highlightedProject ? (
          <p className="notice-banner">
            {`${highlightedProject.repository_name} was created. Open it to continue editing.`}
          </p>
        ) : null}

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
            <MetaItem label="Active targets">{activeBuildTargetCount}</MetaItem>
          ) : null}
        </MetaRow>

        <InputWithPicker
          autoComplete="off"
          buttonIcon="search"
          buttonLabel="Browse"
          className="project-list-toolbar"
          disabled={isLoading || repositories.length === 0}
          hint={
            isLoading
              ? "Loading inventory..."
              : `${filteredRepositories.length} matching project${filteredRepositories.length === 1 ? "" : "s"}`
          }
          label="Quick open"
          leadingIcon="search"
          onChange={setQuickOpenQuery}
          onKeyDown={handleQuickOpenKeyDown}
          onPick={handleQuickOpenPick}
          pickerComponent={SelectListFullScreen}
          pickerProps={{
            description:
              "Search the registered repository inventory and open a project without leaving this screen.",
            emptyStateCopy: "Try a different repository name or remote URL.",
            emptyStateTitle: "No projects matched the current filter.",
            initialQuery: quickOpenQuery,
            items: quickOpenItems,
            title: "Open project",
          }}
          placeholder="Filter by project name or repository URL"
          value={quickOpenQuery}
        />

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

          {!isLoading &&
          repositories.length > 0 &&
          filteredRepositories.length === 0 ? (
            <div className="feed-state">
              <p className="feed-state__title">
                No projects match this filter.
              </p>
              <p className="feed-state__copy">
                Clear or broaden the quick-open query to inspect the rest of the
                repository inventory.
              </p>
            </div>
          ) : null}

          {!isLoading && filteredRepositories.length > 0 ? (
            <ProjectList
              highlightedRepositoryId={highlightedRepositoryId}
              onCardRef={(repositoryId, element) => {
                projectCardRefs.current[repositoryId] = element;
              }}
              onOpen={onOpenProject}
              repositories={filteredRepositories}
            />
          ) : null}
        </SurfacePanel>
      </ScreenScaffold>
    </div>
  );
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

function filterRepositories(
  repositories: RepositoryInspectionEntry[],
  query: string,
) {
  const normalizedQuery = query.trim().toLowerCase();

  if (!normalizedQuery) {
    return repositories;
  }

  return repositories.filter((repository) => {
    return (
      repository.repository_name.toLowerCase().includes(normalizedQuery) ||
      repository.repo_url.toLowerCase().includes(normalizedQuery) ||
      repository.engine_kind.toLowerCase().includes(normalizedQuery)
    );
  });
}

function buildProjectPickerItem(repository: RepositoryInspectionEntry) {
  return {
    id: String(repository.repository_id),
    label: repository.repository_name,
    subtitle: repository.repo_url,
  };
}
