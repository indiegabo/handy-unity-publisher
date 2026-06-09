import type { ReactNode } from "react";

import type { RepositoryInspectionEntry } from "../../services/projects";
import {
  resolveProjectSourceMode,
  type ProjectSourceMode,
} from "../../projectSourcePresentation";
import { LocalWorkspaceStartReleaseAdapter } from "./LocalWorkspaceStartReleaseAdapter";
import { ManagedRepositoryStartReleaseAdapter } from "./ManagedRepositoryStartReleaseAdapter";

export type StartReleaseConfigureAdapterProps = {
  repository: RepositoryInspectionEntry;
  onBack: () => void;
  onCancel: () => void;
  onQueued: (gitTag: string, repositoryName: string) => void;
  onOpenProjects: () => void;
};

type StartReleaseConfigureAdapter = {
  sourceMode: ProjectSourceMode;
  render: (props: StartReleaseConfigureAdapterProps) => ReactNode;
};

const CONFIGURE_ADAPTERS: Record<
  ProjectSourceMode,
  StartReleaseConfigureAdapter
> = {
  local_workspace: {
    sourceMode: "local_workspace",
    render: (props) => (
      <LocalWorkspaceStartReleaseAdapter
        key={props.repository.repository_id}
        onBack={props.onBack}
        onCancel={props.onCancel}
        onQueued={props.onQueued}
        repository={props.repository}
      />
    ),
  },
  managed_repository: {
    sourceMode: "managed_repository",
    render: (props) => (
      <ManagedRepositoryStartReleaseAdapter
        key={props.repository.repository_id}
        onBack={props.onBack}
        onCancel={props.onCancel}
        onQueued={props.onQueued}
        repository={props.repository}
      />
    ),
  },
};

export function resolveStartReleaseConfigureAdapter(
  repository: RepositoryInspectionEntry,
) {
  return CONFIGURE_ADAPTERS[resolveProjectSourceMode(repository)];
}
