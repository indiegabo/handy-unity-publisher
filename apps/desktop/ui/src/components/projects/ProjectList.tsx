import type { RepositoryInspectionEntry } from "../../services/projects";

import ProjectCard from "./ProjectCard";

type ProjectListProps = {
  highlightedRepositoryId?: number | null;
  onCardRef?: (repositoryId: number, element: HTMLButtonElement | null) => void;
  onOpen: (repositoryId: number, repositoryName: string) => void;
  onQuickView: (repositoryId: number) => void;
  repositories: readonly RepositoryInspectionEntry[];
};

export default function ProjectList({
  highlightedRepositoryId = null,
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
          onOpen={onOpen}
          onQuickView={onQuickView}
          ref={(element) => onCardRef?.(repository.repository_id, element)}
          repository={repository}
        />
      ))}
    </div>
  );
}
