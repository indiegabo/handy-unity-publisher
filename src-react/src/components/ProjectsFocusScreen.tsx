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
import { SelectField, TextField, type SelectOption } from "./Field";
import { Icon } from "./Icon";
import ScreenScaffold from "./ScreenScaffold";
import SelectListFullScreen from "./SelectListFullScreen";
import FullScreenModal from "./FullScreenModal";
import ProjectList from "./projects/ProjectList";
import {
  ProjectQuickView,
  type ProjectQuickViewResult,
} from "./ProjectQuickView";
import {
  dispatchOnDemandReleaseProcess,
  loadRepositoryInspection,
  type OnDemandReleaseVersionSource,
  type RepositoryInspectionEntry,
} from "../services/projects";
import {
  buildProjectSourceDisplay,
  buildProjectSourceSearchTerms,
  isLocalWorkspaceSource,
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
  const [, setQuickReleaseMessage] = useState<string | null>(null);
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

  const filteredRepositories = useMemo(
    () => filterRepositories(repositories, deferredQuickOpenQuery),
    [deferredQuickOpenQuery, repositories],
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

  const handleOpenQuickReleaseFlow = useEffectEvent(async () => {
    if (isLoading || repositories.length === 0) {
      return;
    }

    const selectedRepositoryId = await openOverlay<string>(
      SelectListFullScreen,
      {
        description: t(
          "projects.quick_release.selector.description",
          "Choose one registered project to open the quick release start flow.",
        ),
        emptyStateCopy: t(
          "projects.quick_release.selector.empty_copy",
          "Register a project first, then return here to queue a release without opening project detail.",
        ),
        emptyStateTitle: t(
          "projects.quick_release.selector.empty_title",
          "No registered projects available.",
        ),
        items: repositories.map(buildQuickReleaseProjectItem),
        title: t("projects.quick_release.selector.title", "Start release"),
      },
    );

    if (!selectedRepositoryId) {
      return;
    }

    const repository = repositories.find(
      (entry) => String(entry.repository_id) === selectedRepositoryId,
    );

    if (!repository) {
      return;
    }

    const result = await openOverlay<QuickReleaseStartOverlayResult>(
      QuickReleaseStartOverlay,
      {
        repository,
      },
    );

    if (!result) {
      return;
    }

    if (result.kind === "open-project") {
      onOpenProject(repository.repository_id, repository.repository_name);
      return;
    }

    startTransition(() => {
      setQuickReleaseMessage(
        t(
          "projects.quick_release.notice.queued",
          "Queued local release {{gitTag}} for {{repositoryName}}.",
          {
            gitTag: result.gitTag,
            repositoryName: repository.repository_name,
          },
        ),
      );
    });

    await loadProjects("refresh");
  });
  void handleOpenQuickReleaseFlow;

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
        actions={
          <Button
            disabled={isLoading || isRefreshing}
            leadingIcon="refresh"
            onClick={() => void loadProjects("refresh")}
            size="sm"
            variant="secondary"
          >
            {isRefreshing
              ? t("projects.actions.refreshing", "Refreshing...")
              : t("projects.actions.refresh", "Refresh")}
          </Button>
        }
        title={t("projects.title", "Project List")}
      >
        <label className="project-list-toolbar project-list-toolbar--compact ui-field">
          <span className="ui-field__header">
            <span className="ui-field__label" id="projects-filter-label">
              {t("projects.quick_open.label", "Quick open")}
            </span>
          </span>
          <span className="ui-field__control">
            <Icon className="ui-field__icon" name="search" />
            <input
              aria-labelledby="projects-filter-label"
              autoComplete="off"
              className="ui-field__input ui-field__input--with-icon"
              disabled={isLoading || repositories.length === 0}
              onChange={(event) => setQuickOpenQuery(event.currentTarget.value)}
              onKeyDown={handleQuickOpenKeyDown}
              placeholder={t(
                "projects.quick_open.placeholder",
                "Filter by project name, remote URL, or local workspace path",
              )}
              ref={quickOpenInputRef}
              value={quickOpenQuery}
            />
          </span>
        </label>

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

type QuickReleaseDraft = {
  releaseVersion: string;
  versionSource: OnDemandReleaseVersionSource;
};

type QuickReleaseStartOverlayResult =
  | {
      kind: "queued";
      gitTag: string;
    }
  | {
      kind: "open-project";
    };

type QuickReleaseStartOverlayProps = {
  repository: RepositoryInspectionEntry;
  onResolve?: (value?: QuickReleaseStartOverlayResult | null) => void;
};

type QuickReleaseValidationErrors = {
  releaseVersion?: string;
};

const QUICK_RELEASE_VERSION_SOURCE_OPTIONS: SelectOption[] = [
  {
    label: "Manual version label",
    value: "manual",
  },
  {
    label: "Detect from project settings",
    value: "project_settings",
  },
];

function QuickReleaseStartOverlay({
  repository,
  onResolve,
}: QuickReleaseStartOverlayProps) {
  const { t } = useLocalization();
  const [draft, setDraft] = useState<QuickReleaseDraft>({
    releaseVersion: "",
    versionSource: "manual",
  });
  const [validationErrors, setValidationErrors] =
    useState<QuickReleaseValidationErrors>({});
  const [isQueueing, setIsQueueing] = useState(false);
  const [dispatchError, setDispatchError] = useState<string | null>(null);
  const isLocalWorkspace = isLocalWorkspaceSource(repository);

  const handleQueueRelease = async () => {
    if (!isLocalWorkspace || isQueueing) {
      return;
    }

    const nextValidationErrors = validateQuickReleaseDraft(draft);
    if (nextValidationErrors.releaseVersion) {
      setValidationErrors(nextValidationErrors);
      return;
    }

    setIsQueueing(true);
    setDispatchError(null);

    try {
      const release = await dispatchOnDemandReleaseProcess({
        local_path: repository.local_path ?? repository.repo_url,
        release_version:
          draft.versionSource === "manual" ? draft.releaseVersion.trim() : null,
        repository_id: repository.repository_id,
        source_kind: "local_workspace",
        source_ref: null,
        unity_executable_path_override: null,
        version_source: draft.versionSource,
      });

      onResolve?.({
        gitTag: release.git_tag,
        kind: "queued",
      });
    } catch (error) {
      setDispatchError(buildProjectsListErrorMessage(t, error));
      setIsQueueing(false);
    }
  };

  return (
    <FullScreenModal
      description={buildProjectSourceDisplay(repository)}
      dismissible={!isQueueing}
      footer={
        <div className="publish-destination-editor-modal__footer">
          <Button
            disabled={isQueueing}
            onClick={() => onResolve?.(null)}
            size="sm"
            variant="ghost"
          >
            {t("projects.quick_release.actions.cancel", "Cancel")}
          </Button>
          {isLocalWorkspace ? (
            <Button
              disabled={isQueueing}
              leadingIcon="arrowUpRight"
              onClick={() => {
                void handleQueueRelease();
              }}
              size="sm"
              variant="primary"
            >
              {isQueueing
                ? t("projects.quick_release.actions.queueing", "Queueing...")
                : t(
                    "projects.quick_release.actions.queue_local",
                    "Queue Local Release",
                  )}
            </Button>
          ) : (
            <Button
              leadingIcon="arrowUpRight"
              onClick={() => onResolve?.({ kind: "open-project" })}
              size="sm"
              variant="primary"
            >
              {t("projects.quick_release.actions.open_project", "Open project")}
            </Button>
          )}
        </div>
      }
      onResolve={onResolve}
      title={t(
        "projects.quick_release.overlay.title",
        "Start release · {{repositoryName}}",
        {
          repositoryName: repository.repository_name,
        },
      )}
    >
      {isLocalWorkspace ? (
        <div className="project-detail-form-grid">
          <SelectField
            hint={t(
              "projects.quick_release.version_source.hint",
              "Choose whether HGP should use a manual release label or detect it from project settings.",
            )}
            label={t(
              "projects.quick_release.version_source.label",
              "Version source",
            )}
            onChange={(event) =>
              setDraft((current) => ({
                ...current,
                versionSource: event.currentTarget
                  .value as OnDemandReleaseVersionSource,
              }))
            }
            options={QUICK_RELEASE_VERSION_SOURCE_OPTIONS}
            value={draft.versionSource}
          />
          <TextField
            data-overlay-autofocus
            disabled={draft.versionSource !== "manual"}
            error={validationErrors.releaseVersion}
            hint={
              draft.versionSource === "manual"
                ? t(
                    "projects.quick_release.release_version.hint.manual",
                    "Use the release label that should identify this local snapshot.",
                  )
                : t(
                    "projects.quick_release.release_version.hint.detected",
                    "Detected from project settings when the release is queued.",
                  )
            }
            label={t(
              "projects.quick_release.release_version.label",
              "Release version",
            )}
            onChange={(event) =>
              setDraft((current) => ({
                ...current,
                releaseVersion: event.currentTarget.value,
              }))
            }
            placeholder="v1.2.3"
            value={draft.releaseVersion}
          />
          {dispatchError ? (
            <p className="feed-banner feed-banner--error">{dispatchError}</p>
          ) : null}
        </div>
      ) : (
        <div className="feed-state">
          <p className="feed-state__title">
            {t(
              "projects.quick_release.non_local.title",
              "Quick release shortcut is currently available for local workspace projects.",
            )}
          </p>
          <p className="feed-state__copy">
            {t(
              "projects.quick_release.non_local.copy",
              "Open this project detail to queue managed repository releases from Runtime Status.",
            )}
          </p>
        </div>
      )}
    </FullScreenModal>
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

function buildQuickReleaseProjectItem(repository: RepositoryInspectionEntry) {
  return {
    id: String(repository.repository_id),
    label: repository.repository_name,
    subtitle: isLocalWorkspaceSource(repository)
      ? `Local workspace · ${resolveProjectSourceValueForQuickRelease(repository)}`
      : `Managed repository · ${resolveProjectSourceValueForQuickRelease(repository)}`,
  };
}

function resolveProjectSourceValueForQuickRelease(
  repository: RepositoryInspectionEntry,
) {
  if (isLocalWorkspaceSource(repository)) {
    return repository.local_path ?? repository.repo_url;
  }

  return repository.repo_url;
}

function validateQuickReleaseDraft(
  draft: QuickReleaseDraft,
): QuickReleaseValidationErrors {
  if (draft.versionSource !== "manual") {
    return {};
  }

  return draft.releaseVersion.trim()
    ? {}
    : {
        releaseVersion:
          "Release version is required for manual local dispatch.",
      };
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
