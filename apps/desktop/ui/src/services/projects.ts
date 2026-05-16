import { invoke } from "@tauri-apps/api/core";

export type UnityExecutableValidation = {
    runner_family: string;
    unity_executable_path: string | null;
    unity_executable_exists: boolean;
    unity_executable_is_file: boolean;
    additional_argument_count: number;
    environment_variable_count: number;
    status: string;
    message: string;
};

export type RepositoryCredentialReference = {
    credential_id: number;
    name: string;
    kind: string;
    config_status: string;
    config_message: string;
};

export type RepositoryPublishTargetInspection = {
    publish_target_id: number;
    name: string;
    kind: string;
    enabled: boolean;
    credentials: RepositoryCredentialReference | null;
};

export type ReleaseAutomationStatus = {
    release_run_id: number;
    git_tag: string;
    unity_version: string | null;
    status: string;
    planned: boolean;
    build_process_active: boolean;
    publish_process_active: boolean;
    queued_build_runs: number;
    running_build_runs: number;
    terminal_build_runs: number;
    total_build_runs: number;
    queued_publish_runs: number;
    running_publish_runs: number;
    terminal_publish_runs: number;
    total_publish_runs: number;
};

export type UnityBuildTargetRunnerSettings = {
    build_target_id: number;
    repository_id: number;
    repository_name: string;
    target_name: string;
    platform: string;
    runner_type: string;
    build_method: string | null;
    enabled: boolean;
    diagnostic_status: string;
    diagnostic_message: string;
    host_native_diagnostics: UnityExecutableValidation | null;
};

export type RepositoryInspectionEntry = {
    repository_id: number;
    repository_name: string;
    repo_url: string;
    enabled: boolean;
    polling_interval_seconds: number;
    default_branch: string | null;
    artifacts_root_override: string | null;
    workspace_root_override: string | null;
    last_seen_tag: string | null;
    enabled_build_target_count: number;
    credentials: RepositoryCredentialReference | null;
    build_targets: UnityBuildTargetRunnerSettings[];
    publish_targets: RepositoryPublishTargetInspection[];
    pending_release_count: number;
    queued_build_runs: number;
    running_build_runs: number;
    queued_publish_runs: number;
    running_publish_runs: number;
    release_queue: ReleaseAutomationStatus[];
};

export type RepositoryInspectionSettings = {
    generated_at: string;
    repositories: RepositoryInspectionEntry[];
};

export type HostPathSelectionKind = "file" | "directory";

export type PickHostPathFilter = {
    name: string;
    extensions: string[];
};

export type PickHostPathInput = {
    kind: HostPathSelectionKind;
    title?: string;
    filters?: PickHostPathFilter[];
};

export type CreateRepositoryProjectBuildTargetInput = {
    name: string;
    platform: string;
    build_method: string;
    unity_executable_path: string;
};

export type CreateRepositoryProjectInput = {
    name: string;
    repository_url: string;
    default_branch?: string | null;
    artifacts_root_override?: string | null;
    workspace_root_override?: string | null;
    polling_interval_seconds: number;
    build_targets: CreateRepositoryProjectBuildTargetInput[];
};

export type UpdateRepositoryProjectBuildTargetInput = {
    build_target_id?: number | null;
    name: string;
    platform: string;
    build_method: string;
    unity_executable_path: string;
};

export type CreatedRepositoryProjectRecord = {
    repository_id: number;
    repository_name: string;
    credentials_id: number | null;
    build_target_ids: number[];
};

export type UpdateRepositoryProjectInput = {
    repository_id: number;
    name: string;
    repository_url: string;
    default_branch?: string | null;
    artifacts_root_override?: string | null;
    workspace_root_override?: string | null;
    polling_interval_seconds: number;
    enabled: boolean;
    build_targets: UpdateRepositoryProjectBuildTargetInput[];
};

export async function loadRepositoryInspection(): Promise<RepositoryInspectionSettings> {
    return invoke<RepositoryInspectionSettings>("repository_inspection");
}

export async function pickHostPath(
    input: PickHostPathInput,
): Promise<string | null> {
    return invoke<string | null>("pick_host_path", {
        input,
    });
}

export async function pickUnityExecutablePath(): Promise<string | null> {
    return invoke<string | null>("pick_unity_executable_path");
}

export async function validateUnityExecutablePath(
    path: string,
): Promise<UnityExecutableValidation> {
    return invoke<UnityExecutableValidation>("validate_unity_executable_path", {
        path,
    });
}

export async function createRepositoryProject(
    input: CreateRepositoryProjectInput,
): Promise<CreatedRepositoryProjectRecord> {
    return invoke<CreatedRepositoryProjectRecord>("create_repository_project", {
        input,
    });
}

export async function updateRepositoryProject(
    input: UpdateRepositoryProjectInput,
): Promise<void> {
    return invoke<void>("update_repository_project", {
        input,
    });
}