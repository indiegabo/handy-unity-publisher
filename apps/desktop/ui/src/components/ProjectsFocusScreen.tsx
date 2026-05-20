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
import { MetaItem, MetaRow } from "./Surface";
import ScreenScaffold from "./ScreenScaffold";
import InputWithPicker from "./InputWithPicker";
import SelectListFullScreen from "./SelectListFullScreen";
import ProjectList from "./projects/ProjectList";
import {
  ProjectQuickView,
  type ProjectQuickViewResult,
} from "./ProjectQuickView";
import {
  loadRepositoryInspection,
  type RepositoryInspectionEntry,
} from "../services/projects";
import { useOverlay } from "./OverlayManager";

type ProjectsFocusScreenProps = {
  highlightedRepositoryId?: number | null;
  onOpenProject: (repositoryId: number, repositoryName: string) => void;
};

export function ProjectsFocusScreen({
  highlightedRepositoryId = null,
  onOpenProject,
}: ProjectsFocusScreenProps) {
  const { openOverlay } = useOverlay();
  const [repositories, setRepositories] = useState<RepositoryInspectionEntry[]>(
    [],
  );
  const [isLoading, setIsLoading] = useState(true);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [quickOpenQuery, setQuickOpenQuery] = useState("");
  const quickOpenInputRef = useRef<HTMLInputElement | null>(null);
  const projectCardRefs = useRef<Record<number, HTMLButtonElement | null>>({});
  const deferredQuickOpenQuery = useDeferredValue(quickOpenQuery);

  const focusProjectCard = useEffectEvent((repositoryId: number) => {
    const targetCard = projectCardRefs.current[repositoryId];

    if (!targetCard) {
      return;
    }

    targetCard.focus();

    if (document.activeElement !== targetCard) {
      window.requestAnimationFrame(() => {
        projectCardRefs.current[repositoryId]?.focus();
      });
    }
  });

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

  const loadProjects = useEffectEvent(async (reason: "initial" | "refresh") => {
    if (reason === "refresh" && repositories.length > 0) {
      setIsRefreshing(true);
    } else {
      setIsLoading(true);
    }

    try {
      const inspection = await loadRepositoryInspection();

      startTransition(() => {
        setRepositories(inspection.repositories);
        setError(null);
        setIsLoading(false);
        setIsRefreshing(false);
      });
    } catch (loadError) {
      startTransition(() => {
        setError(buildProjectsListErrorMessage(loadError));
        setIsLoading(false);
        setIsRefreshing(false);
      });
    }
  });

  useEffect(() => {
    void loadProjects("initial");
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

  const handleOpenProjectQuickView = useEffectEvent(
    async (repositoryId: number) => {
      const repository = repositories.find(
        (entry) => entry.repository_id === repositoryId,
      );

      if (!repository) {
        return;
      }

      const result = await openOverlay<ProjectQuickViewResult>(
        ProjectQuickView,
        {
          repository,
        },
      );

      if (result === "open-project") {
        onOpenProject(repository.repository_id, repository.repository_name);
      }
    },
  );

  const handleQuickOpenKeyDown = useEffectEvent(
    (event: React.KeyboardEvent<HTMLInputElement>) => {
      if (event.key === "ArrowDown" && filteredRepositories.length > 0) {
        event.preventDefault();
        focusProjectCard(filteredRepositories[0].repository_id);
        return;
      }

      if (event.key === "ArrowUp" && filteredRepositories.length > 0) {
        event.preventDefault();
        focusProjectCard(
          filteredRepositories[filteredRepositories.length - 1].repository_id,
        );
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

  const handleProjectCardKeyDown = useEffectEvent(
    (repositoryId: number, event: React.KeyboardEvent<HTMLButtonElement>) => {
      if (filteredRepositories.length === 0) {
        return;
      }

      if (event.key === "Escape") {
        event.preventDefault();
        quickOpenInputRef.current?.focus();
        return;
      }

      const currentIndex = filteredRepositories.findIndex(
        (repository) => repository.repository_id === repositoryId,
      );

      if (currentIndex === -1) {
        return;
      }

      let targetRepositoryId = repositoryId;

      switch (event.key) {
        case "ArrowDown":
          targetRepositoryId =
            filteredRepositories[
              Math.min(currentIndex + 1, filteredRepositories.length - 1)
            ].repository_id;
          break;
        case "ArrowUp":
          targetRepositoryId =
            filteredRepositories[Math.max(currentIndex - 1, 0)].repository_id;
          break;
        case "Home":
          targetRepositoryId = filteredRepositories[0].repository_id;
          break;
        case "End":
          targetRepositoryId =
            filteredRepositories[filteredRepositories.length - 1].repository_id;
          break;
        default:
          return;
      }

      event.preventDefault();
      focusProjectCard(targetRepositoryId);
    },
  );

  return (
    <div className="project-list-shell">
      <ScreenScaffold
        eyebrow="Projects"
        title="Project List"
        subtitle="Browse registered repositories, inspect current automation health, and jump into project editing without losing context."
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
        actions={
          <Button
            leadingIcon="refresh"
            disabled={isLoading || isRefreshing}
            onClick={() => void loadProjects("refresh")}
            size="sm"
            variant="secondary"
          >
            {isRefreshing ? "Refreshing..." : "Refresh"}
          </Button>
        }
      >
        {highlightedProject ? (
          <p className="notice-banner">
            {`${highlightedProject.repository_name} was created. Open it to continue editing.`}
          </p>
        ) : null}

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
          inputRef={quickOpenInputRef}
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

        {error && !isLoading && repositories.length > 0 ? (
          <div className="project-list-state">
            <p className="feed-banner feed-banner--error">{error}</p>
            <div className="project-list-state__actions">
              <Button
                leadingIcon="refresh"
                onClick={() => void loadProjects("refresh")}
                size="sm"
                variant="ghost"
              >
                Retry load
              </Button>
            </div>
          </div>
        ) : null}

        {isRefreshing && repositories.length > 0 ? (
          <p className="notice-banner">
            Refreshing repository inventory while keeping the latest known
            snapshot visible.
          </p>
        ) : null}

        {isLoading ? (
          <div className="feed-state">
            <p className="feed-state__title">Loading projects...</p>
            <p className="feed-state__copy">
              The shell is resolving the latest repository inspection snapshot.
            </p>
          </div>
        ) : null}

        {!isLoading && error && repositories.length === 0 ? (
          <div className="feed-state project-list-state">
            <p className="feed-state__title">Could not load projects.</p>
            <p className="feed-state__copy">{error}</p>
            <div className="project-list-state__actions">
              <Button
                leadingIcon="refresh"
                onClick={() => void loadProjects("refresh")}
                size="sm"
                variant="secondary"
              >
                Retry load
              </Button>
            </div>
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
            <p className="feed-state__title">No projects match this filter.</p>
            <p className="feed-state__copy">
              Clear or broaden the quick-open query to inspect the rest of the
              repository inventory.
            </p>
          </div>
        ) : null}

        {!isLoading && filteredRepositories.length > 0 ? (
          <ProjectList
            highlightedRepositoryId={highlightedRepositoryId}
            onCardKeyDown={handleProjectCardKeyDown}
            onCardRef={(repositoryId, element) => {
              projectCardRefs.current[repositoryId] = element;
            }}
            onOpen={onOpenProject}
            onQuickView={handleOpenProjectQuickView}
            repositories={filteredRepositories}
          />
        ) : null}
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
