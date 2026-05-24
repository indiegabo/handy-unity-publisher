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
import {
  buildProjectSourceDisplay,
  buildProjectSourceSearchTerms,
} from "../projectSourcePresentation";
import { useLocalization } from "../LocalizationProvider";
import { useOverlay } from "./OverlayManager";

type ProjectsFocusScreenProps = {
  highlightedRepositoryId?: number | null;
  onOpenProject: (repositoryId: number, repositoryName: string) => void;
};

export function ProjectsFocusScreen({
  highlightedRepositoryId = null,
  onOpenProject,
}: ProjectsFocusScreenProps) {
  const { t } = useLocalization();
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
        setError(buildProjectsListErrorMessage(t, loadError));
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
        const sourceTerms = buildProjectSourceSearchTerms(repository).map(
          (term) => term.toLowerCase(),
        );

        return (
          repository.repository_name.toLowerCase() === normalizedQuery ||
          sourceTerms.includes(normalizedQuery)
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
        eyebrow={t("projects.eyebrow", "Projects")}
        title={t("projects.title", "Project List")}
        subtitle={t(
          "projects.subtitle",
          "Browse registered repositories, inspect current automation health, and jump into project editing without losing context.",
        )}
        summary={
          <MetaRow>
            <MetaItem label={t("projects.summary.projects", "Projects")}>
              {isLoading
                ? t("projects.summary.loading", "Loading snapshot...")
                : t("projects.summary.registered", "{{count}} registered", {
                    count: repositories.length,
                  })}
            </MetaItem>
            {!isLoading ? (
              <MetaItem label={t("projects.summary.enabled", "Enabled")}>
                {enabledRepositoryCount}
              </MetaItem>
            ) : null}
            {!isLoading ? (
              <MetaItem label={t("projects.summary.disabled", "Disabled")}>
                {disabledRepositoryCount}
              </MetaItem>
            ) : null}
            {!isLoading ? (
              <MetaItem
                label={t("projects.summary.active_targets", "Active targets")}
              >
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
            {isRefreshing
              ? t("projects.actions.refreshing", "Refreshing...")
              : t("projects.actions.refresh", "Refresh")}
          </Button>
        }
      >
        {highlightedProject ? (
          <p className="notice-banner">
            {t(
              "projects.notice.created",
              "{{repositoryName}} was created. Open it to continue editing.",
              {
                repositoryName: highlightedProject.repository_name,
              },
            )}
          </p>
        ) : null}

        <InputWithPicker
          autoComplete="off"
          buttonIcon="search"
          buttonLabel={t("projects.quick_open.browse", "Browse")}
          className="project-list-toolbar"
          disabled={isLoading || repositories.length === 0}
          hint={
            isLoading
              ? t("projects.quick_open.loading", "Loading inventory...")
              : filteredRepositories.length === 1
                ? t("projects.quick_open.matching.one", "1 matching project")
                : t(
                    "projects.quick_open.matching.other",
                    "{{count}} matching projects",
                    { count: filteredRepositories.length },
                  )
          }
          inputRef={quickOpenInputRef}
          label={t("projects.quick_open.label", "Quick open")}
          leadingIcon="search"
          onChange={setQuickOpenQuery}
          onKeyDown={handleQuickOpenKeyDown}
          onPick={handleQuickOpenPick}
          pickerComponent={SelectListFullScreen}
          pickerProps={{
            description: t(
              "projects.quick_open.picker.description",
              "Search the registered project inventory and open a project without leaving this screen.",
            ),
            emptyStateCopy: t(
              "projects.quick_open.picker.empty_copy",
              "Try a different project name, remote URL, or local workspace path.",
            ),
            emptyStateTitle: t(
              "projects.quick_open.picker.empty_title",
              "No projects matched the current filter.",
            ),
            initialQuery: quickOpenQuery,
            items: quickOpenItems,
            title: t("projects.quick_open.picker.title", "Open project"),
          }}
          placeholder={t(
            "projects.quick_open.placeholder",
            "Filter by project name, remote URL, or local workspace path",
          )}
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
                {t("projects.actions.retry", "Retry load")}
              </Button>
            </div>
          </div>
        ) : null}

        {isRefreshing && repositories.length > 0 ? (
          <p className="notice-banner">
            {t(
              "projects.notice.refreshing",
              "Refreshing repository inventory while keeping the latest known snapshot visible.",
            )}
          </p>
        ) : null}

        {isLoading ? (
          <div className="feed-state">
            <p className="feed-state__title">
              {t("projects.loading.title", "Loading projects...")}
            </p>
            <p className="feed-state__copy">
              {t(
                "projects.loading.copy",
                "The shell is resolving the latest project inspection snapshot.",
              )}
            </p>
          </div>
        ) : null}

        {!isLoading && error && repositories.length === 0 ? (
          <div className="feed-state project-list-state">
            <p className="feed-state__title">
              {t("projects.error.title", "Could not load projects.")}
            </p>
            <p className="feed-state__copy">{error}</p>
            <div className="project-list-state__actions">
              <Button
                leadingIcon="refresh"
                onClick={() => void loadProjects("refresh")}
                size="sm"
                variant="secondary"
              >
                {t("projects.actions.retry", "Retry load")}
              </Button>
            </div>
          </div>
        ) : null}

        {!isLoading && repositories.length === 0 ? (
          <div className="feed-state">
            <p className="feed-state__title">
              {t(
                "projects.empty.none_configured.title",
                "No projects configured yet.",
              )}
            </p>
            <p className="feed-state__copy">
              {t(
                "projects.empty.none_configured.copy",
                "Create a repository project from the home screen to manage it here.",
              )}
            </p>
          </div>
        ) : null}

        {!isLoading &&
        repositories.length > 0 &&
        filteredRepositories.length === 0 ? (
          <div className="feed-state">
            <p className="feed-state__title">
              {t(
                "projects.empty.no_match.title",
                "No projects match this filter.",
              )}
            </p>
            <p className="feed-state__copy">
              {t(
                "projects.empty.no_match.copy",
                "Clear or broaden the quick-open query to inspect the rest of the project inventory.",
              )}
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

function buildProjectsListErrorMessage(
  t: ReturnType<typeof useLocalization>["t"],
  error: unknown,
) {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }

  return t(
    "projects.error.fallback",
    "The desktop shell could not load the project list.",
  );
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
    const sourceTerms = buildProjectSourceSearchTerms(repository);

    return (
      repository.repository_name.toLowerCase().includes(normalizedQuery) ||
      sourceTerms.some((term) =>
        term.toLowerCase().includes(normalizedQuery),
      ) ||
      repository.engine_kind.toLowerCase().includes(normalizedQuery)
    );
  });
}

function buildProjectPickerItem(repository: RepositoryInspectionEntry) {
  return {
    id: String(repository.repository_id),
    label: repository.repository_name,
    subtitle: buildProjectSourceDisplay(repository),
  };
}
