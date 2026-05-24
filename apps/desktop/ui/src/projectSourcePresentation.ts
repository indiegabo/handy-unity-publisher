import type { RepositoryInspectionEntry } from "./services/projects";

export type ProjectSourceMode = "managed_repository" | "local_workspace";
export type ProjectPresentationTranslate = (
    key: string,
    fallback: string,
    values?: Record<string, string | number>,
) => string;

type ProjectSourceInput = {
    source_mode?: string | null;
    sourceMode?: string | null;
    repo_url?: string | null;
    repositoryUrl?: string | null;
    local_path?: string | null;
    localPath?: string | null;
};

export function resolveProjectSourceMode(
    input: ProjectSourceInput,
): ProjectSourceMode {
    const sourceMode = (input.sourceMode ?? input.source_mode ?? "").trim();

    return sourceMode === "local_workspace"
        ? "local_workspace"
        : "managed_repository";
}

export function isLocalWorkspaceSource(input: ProjectSourceInput) {
    return resolveProjectSourceMode(input) === "local_workspace";
}

export function isManagedRepositorySource(input: ProjectSourceInput) {
    return resolveProjectSourceMode(input) === "managed_repository";
}

export function resolveProjectSourceLabel(input: ProjectSourceInput) {
    return isLocalWorkspaceSource(input)
        ? "Local workspace"
        : "Managed repository";
}

export function resolveLocalizedProjectSourceLabel(
    translate: ProjectPresentationTranslate,
    input: ProjectSourceInput,
) {
    return isLocalWorkspaceSource(input)
        ? translate(
            "projects.presentation.source.local_workspace",
            "Local workspace",
        )
        : translate(
            "projects.presentation.source.managed_repository",
            "Managed repository",
        );
}

export function resolveProjectSourceFieldLabel(input: ProjectSourceInput) {
    return isLocalWorkspaceSource(input)
        ? "Local workspace path"
        : "Repository URL";
}

export function resolveLocalizedProjectSourceFieldLabel(
    translate: ProjectPresentationTranslate,
    input: ProjectSourceInput,
) {
    return isLocalWorkspaceSource(input)
        ? translate(
            "projects.presentation.field.local_workspace_path",
            "Local workspace path",
        )
        : translate(
            "projects.presentation.field.repository_url",
            "Repository URL",
        );
}

export function resolveProjectSourceValue(input: ProjectSourceInput) {
    if (isLocalWorkspaceSource(input)) {
        return normalizeSourceString(
            input.localPath ??
            input.local_path ??
            input.repositoryUrl ??
            input.repo_url ??
            "",
        );
    }

    return normalizeSourceString(
        input.repositoryUrl ??
        input.repo_url ??
        input.localPath ??
        input.local_path ??
        "",
    );
}

export function buildProjectSourceDisplay(input: ProjectSourceInput) {
    const sourceValue = resolveProjectSourceValue(input);

    return sourceValue
        ? `${resolveProjectSourceLabel(input)} · ${sourceValue}`
        : resolveProjectSourceLabel(input);
}

export function buildLocalizedProjectSourceDisplay(
    translate: ProjectPresentationTranslate,
    input: ProjectSourceInput,
) {
    const sourceValue = resolveProjectSourceValue(input);
    const sourceLabel = resolveLocalizedProjectSourceLabel(translate, input);

    return sourceValue
        ? translate(
            "projects.presentation.source.display",
            "{{sourceLabel}} · {{sourceValue}}",
            {
                sourceLabel,
                sourceValue,
            },
        )
        : sourceLabel;
}

export function buildProjectSourceSearchTerms(input: ProjectSourceInput) {
    return [
        resolveProjectSourceLabel(input),
        resolveProjectSourceValue(input),
        normalizeSourceString(input.repositoryUrl ?? input.repo_url ?? ""),
        normalizeSourceString(input.localPath ?? input.local_path ?? ""),
    ].filter(Boolean);
}

export function resolveProjectSourceModeSummary(input: ProjectSourceInput) {
    return isLocalWorkspaceSource(input)
        ? "Direct workspace"
        : "Managed checkout";
}

export function resolveLocalizedProjectSourceModeSummary(
    translate: ProjectPresentationTranslate,
    input: ProjectSourceInput,
) {
    return isLocalWorkspaceSource(input)
        ? translate(
            "projects.presentation.mode.direct_workspace",
            "Direct workspace",
        )
        : translate(
            "projects.presentation.mode.managed_checkout",
            "Managed checkout",
        );
}

export function resolveProjectAutomationCadenceLabel(
    repository: RepositoryInspectionEntry,
) {
    return isLocalWorkspaceSource(repository)
        ? "No remote polling"
        : `${repository.polling_interval_seconds}s cadence`;
}

export function resolveLocalizedProjectAutomationCadenceLabel(
    translate: ProjectPresentationTranslate,
    repository: RepositoryInspectionEntry,
) {
    return isLocalWorkspaceSource(repository)
        ? translate(
            "projects.presentation.sync.no_remote_polling",
            "No remote polling",
        )
        : translate(
            "projects.presentation.sync.cadence",
            "{{seconds}}s cadence",
            {
                seconds: repository.polling_interval_seconds,
            },
        );
}

function normalizeSourceString(value: string | null | undefined) {
    return typeof value === "string" ? value.trim() : "";
}