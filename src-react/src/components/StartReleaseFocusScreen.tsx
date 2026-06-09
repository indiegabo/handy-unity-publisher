import { useDeferredValue, useMemo, useRef, useState } from "react";

import { TextField } from "./Field";
import ScreenScaffold from "./ScreenScaffold";
import { useLocalization } from "../LocalizationProvider";
import { type RepositoryInspectionEntry } from "../services/projects";
import { buildProjectSourceDisplay } from "../projectSourcePresentation";
import { resolveStartReleaseConfigureAdapter } from "./startRelease/configureAdapter";

type Phase =
  | { kind: "select" }
  | { kind: "configure"; repository: RepositoryInspectionEntry };

export type StartReleaseFocusScreenProps = {
  repositories: RepositoryInspectionEntry[];
  onBack: () => void;
  onQueued: (gitTag: string, repositoryName: string) => void;
  onOpenProjects: () => void;
};

export function StartReleaseFocusScreen({
  repositories,
  onBack,
  onQueued,
  onOpenProjects,
}: StartReleaseFocusScreenProps) {
  const [phase, setPhase] = useState<Phase>({ kind: "select" });

  if (phase.kind === "configure") {
    const adapter = resolveStartReleaseConfigureAdapter(phase.repository);

    return adapter.render({
      repository: phase.repository,
      onBack: () => setPhase({ kind: "select" }),
      onCancel: onBack,
      onQueued,
      onOpenProjects,
    });
  }

  return (
    <SelectPhase
      repositories={repositories}
      onSelect={(repository) => setPhase({ kind: "configure", repository })}
    />
  );
}

type SelectPhaseProps = {
  repositories: RepositoryInspectionEntry[];
  onSelect: (repository: RepositoryInspectionEntry) => void;
};

function SelectPhase({ repositories, onSelect }: SelectPhaseProps) {
  const { t } = useLocalization();
  const [query, setQuery] = useState("");
  const deferredQuery = useDeferredValue(query);
  const itemRefs = useRef<Array<HTMLButtonElement | null>>([]);

  const filtered = useMemo(() => {
    const normalized = deferredQuery.trim().toLowerCase();
    if (!normalized) {
      return repositories;
    }

    return repositories.filter((repository) => {
      const display = buildProjectSourceDisplay(repository).toLowerCase();
      return (
        repository.repository_name.toLowerCase().includes(normalized) ||
        display.includes(normalized)
      );
    });
  }, [deferredQuery, repositories]);

  const resultCountHint =
    filtered.length === 1
      ? t("start_release.select.results.one", "1 result")
      : t("start_release.select.results.other", "{{count}} results", {
          count: filtered.length,
        });

  return (
    <ScreenScaffold
      eyebrow={t("start_release.eyebrow", "Release")}
      subtitle={t(
        "start_release.select.subtitle",
        "Choose a project to queue a release.",
      )}
      title={t("start_release.select.title", "Start release")}
    >
      <div className="start-release-screen__body">
        <TextField
          autoComplete="off"
          hint={resultCountHint}
          label={t("start_release.select.filter.label", "Filter projects")}
          leadingIcon="search"
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown" && filtered.length > 0) {
              event.preventDefault();
              itemRefs.current[0]?.focus();
            }
          }}
          placeholder={t(
            "start_release.select.filter.placeholder",
            "Search by name or source path",
          )}
          value={query}
        />

        <div
          aria-label={t("start_release.select.list.aria_label", "Project list")}
          className="select-list-modal__list"
          role="list"
        >
          {repositories.length === 0 ? (
            <div className="feed-state start-release-screen__empty">
              <p className="feed-state__title">
                {t(
                  "start_release.select.empty.no_projects.title",
                  "No registered projects.",
                )}
              </p>
              <p className="feed-state__copy">
                {t(
                  "start_release.select.empty.no_projects.copy",
                  "Create a project first, then return here to queue a release.",
                )}
              </p>
            </div>
          ) : filtered.length === 0 ? (
            <div className="feed-state start-release-screen__empty">
              <p className="feed-state__title">
                {t(
                  "start_release.select.empty.no_results.title",
                  "No results matched the filter.",
                )}
              </p>
              <p className="feed-state__copy">
                {t(
                  "start_release.select.empty.no_results.copy",
                  "Try a broader search term or clear the filter.",
                )}
              </p>
            </div>
          ) : (
            filtered.map((repository, index) => (
              <button
                className="select-list-modal__item"
                key={repository.repository_id}
                onClick={() => onSelect(repository)}
                onKeyDown={(event) => {
                  if (event.key === "ArrowDown") {
                    event.preventDefault();
                    itemRefs.current[
                      Math.min(index + 1, filtered.length - 1)
                    ]?.focus();
                    return;
                  }

                  if (event.key === "ArrowUp") {
                    event.preventDefault();

                    if (index === 0) {
                      const container =
                        event.currentTarget.closest(".screen-scaffold");
                      const input =
                        container?.querySelector<HTMLElement>("input");
                      input?.focus();
                      return;
                    }

                    itemRefs.current[index - 1]?.focus();
                    return;
                  }

                  if (event.key === "Home") {
                    event.preventDefault();
                    itemRefs.current[0]?.focus();
                    return;
                  }

                  if (event.key === "End") {
                    event.preventDefault();
                    itemRefs.current[filtered.length - 1]?.focus();
                  }
                }}
                ref={(element) => {
                  itemRefs.current[index] = element;
                }}
                type="button"
              >
                <span className="select-list-modal__item-content">
                  <span className="select-list-modal__item-label">
                    {repository.repository_name}
                  </span>
                  <span className="select-list-modal__item-copy">
                    {buildProjectSourceDisplay(repository)}
                  </span>
                </span>
                <span className="select-list-modal__item-action">
                  {t("start_release.select.actions.select", "Select")}
                </span>
              </button>
            ))
          )}
        </div>
      </div>
    </ScreenScaffold>
  );
}
