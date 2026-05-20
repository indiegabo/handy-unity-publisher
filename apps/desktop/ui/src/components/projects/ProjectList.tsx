import type { RepositoryInspectionEntry } from "../../services/projects";

import ProjectCard from "./ProjectCard";

type ProjectListProps = {
  onCardKeyDown?: (
    repositoryId: number,
    event: React.KeyboardEvent<HTMLButtonElement>,
  ) => void;
  highlightedRepositoryId?: number | null;
  onCardRef?: (repositoryId: number, element: HTMLButtonElement | null) => void;
  onOpen: (repositoryId: number, repositoryName: string) => void;
  onQuickView: (repositoryId: number) => void;
  repositories: readonly RepositoryInspectionEntry[];
};

export default function ProjectList({
  highlightedRepositoryId = null,
  onCardKeyDown,
  onCardRef,
  onOpen,
  onQuickView,
  repositories,
}: ProjectListProps) {
  return (
    <div className="project-list-grid">
      {repositories.map((repository) => (
        <ProjectCard
          highlighted={repository.repository_id === highlightedRepositoryId}
          key={repository.repository_id}
          onCardKeyDown={onCardKeyDown}
          onOpen={onOpen}
          onQuickView={onQuickView}
          ref={(element) => onCardRef?.(repository.repository_id, element)}
          repository={repository}
        />
      ))}
    </div>
  );
}
