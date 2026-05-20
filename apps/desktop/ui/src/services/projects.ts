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

export type RepositoryPublishBindingConsumptionBehavior =
    | "consuming"
    | "non_consuming";

export type RepositoryPublishBindingInspection = {
    build_target_id: number;
    build_target_name: string;
    enabled: boolean;
    options_json: string;
    consumption_behavior: RepositoryPublishBindingConsumptionBehavior;
};

export type RepositoryPublishTargetInspection = {
    publish_target_id: number;
    name: string;
    kind: string;
    enabled: boolean;
    config_json: string;
    credentials: RepositoryCredentialReference | null;
    bindings: RepositoryPublishBindingInspection[];
};

export type ReleaseAutomationStatus = {
    release_run_id: number;
    git_tag: string;
    engine_version: string | null;
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

export type UnityAdapterBuildTargetSettings = {
    build_target_id: number;
    repository_id: number;
    repository_name: string;
    target_name: string;
    unity_target_platform: string;
    runner_type: string;
    unity_build_method: string | null;
    enabled: boolean;
    diagnostic_status: string;
    diagnostic_message: string;
    host_native_diagnostics: UnityExecutableValidation | null;
};

export type RepositoryEngineKind =
    | "unity"
    | "unreal"
    | "godot"
    | "gamemaker"
    | "defold"
    | "cocos-creator";

export type UnityBuildContractInput = {
    target_platform: string;
    build_method: string;
};

export type BuildContractInput = {
    unity?: UnityBuildContractInput | null;
};

export type RepositoryInspectionEntry = {
    repository_id: number;
    repository_name: string;
    repo_url: string;
    engine_kind: string;
    enabled: boolean;
    polling_interval_seconds: number;
    default_branch: string | null;
    artifacts_root_override: string | null;
    workspace_root_override: string | null;
    last_seen_tag: string | null;
    enabled_build_target_count: number;
    credentials: RepositoryCredentialReference | null;
    source_provider_id: string | null;
    source_instance_url: string | null;
    visibility_status: string;
    auth_requirement_status: string;
    auth_binding_status: string;
    auth_status_message: string;
    auth_last_verified_at: string | null;
    build_targets: UnityAdapterBuildTargetSettings[];
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

export type RepositoryAccessAssessment = {
    provider_id: string;
    provider_label: string;
    instance_url: string;
    normalized_url: string;
    visibility: string;
    auth_requirement: string;
    auth_status: string;
    supports_interactive_login: boolean;
    message: string;
};

export type RepositoryProviderDetection = {
    provider_id: string;
    provider_label: string;
    instance_url: string;
    normalized_url: string;
    supports_interactive_login: boolean;
};

export type CredentialConfigSummary = {
    status: string;
    message: string;
    top_level_keys: string[];
    missing_required_keys: string[];
};

export type SecretCredentialSetting = {
    credential_id: number;
    name: string;
    kind: string;
    created_at: string;
    updated_at: string;
    storage_model: string;
    config_summary: CredentialConfigSummary;
};

export type SecretSettings = {
    storage_model: string;
    supported_credential_kinds: string[];
    warnings: string[];
    credentials: SecretCredentialSetting[];
};

export type SecretCredentialKind =
    | "git-http-basic"
    | "git-http-bearer"
    | "git-http-github-host-login"
    | "itch-api-key";

export type SaveSecretCredentialInput = {
    credential_id?: number | null;
    name: string;
    kind: SecretCredentialKind;
    config_json: string;
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
    contract: BuildContractInput;
    unity_executable_path: string;
};

export type CreateRepositoryProjectPublishBindingInput = {
    build_target_name: string;
    enabled: boolean;
    options_json: string;
};

export type CreateRepositoryProjectPublishTargetInput = {
    name: string;
    kind: string;
    enabled: boolean;
    config_json: string;
    credentials_id?: number | null;
    bindings: CreateRepositoryProjectPublishBindingInput[];
};

export type CreateRepositoryProjectInput = {
    name: string;
    engine_kind: RepositoryEngineKind;
    repository_url: string;
    repository_access_assessment?: RepositoryAccessAssessment | null;
    repository_credentials_id?: number | null;
    default_branch?: string | null;
    artifacts_root_override?: string | null;
    workspace_root_override?: string | null;
    polling_interval_seconds: number;
    build_targets: CreateRepositoryProjectBuildTargetInput[];
    publish_targets: CreateRepositoryProjectPublishTargetInput[];
};

export type UpdateRepositoryProjectBuildTargetInput = {
    build_target_id?: number | null;
    name: string;
    contract: BuildContractInput;
    unity_executable_path: string;
};

export type UpdateRepositoryProjectPublishBindingInput = {
    build_target_id?: number | null;
    build_target_name: string;
    enabled: boolean;
    options_json: string;
};

export type CreatedRepositoryProjectRecord = {
    repository_id: number;
    repository_name: string;
    credentials_id: number | null;
    build_target_ids: number[];
};

export type UpdateRepositoryProjectPublishTargetInput = {
    publish_target_id?: number | null;
    name: string;
    kind: string;
    enabled: boolean;
    config_json: string;
    credentials_id?: number | null;
    bindings: UpdateRepositoryProjectPublishBindingInput[];
};

export type UpdateRepositoryProjectInput = {
    repository_id: number;
    name: string;
    engine_kind: RepositoryEngineKind;
    repository_url: string;
    repository_access_assessment?: RepositoryAccessAssessment | null;
    default_branch?: string | null;
    artifacts_root_override?: string | null;
    workspace_root_override?: string | null;
    polling_interval_seconds: number;
    enabled: boolean;
    build_targets: UpdateRepositoryProjectBuildTargetInput[];
    publish_targets: UpdateRepositoryProjectPublishTargetInput[];
};

export async function loadRepositoryInspection(): Promise<RepositoryInspectionSettings> {
    return invoke<RepositoryInspectionSettings>("repository_inspection");
}

export async function loadRepositoryProjectDetail(
    repositoryId: number,
): Promise<RepositoryInspectionEntry> {
    return invoke<RepositoryInspectionEntry>("repository_project_detail", {
        repositoryId,
    });
}

export async function assessRepositoryAccess(
    repositoryUrl: string,
): Promise<RepositoryAccessAssessment> {
    return invoke<RepositoryAccessAssessment>("assess_repository_access", {
        input: {
            repository_url: repositoryUrl,
        },
    });
}

export async function detectRepositoryProvider(
    repositoryUrl: string,
): Promise<RepositoryProviderDetection> {
    return invoke<RepositoryProviderDetection>("detect_repository_provider", {
        input: {
            repository_url: repositoryUrl,
        },
    });
}

export async function loadSecretSettings(): Promise<SecretSettings> {
    return invoke<SecretSettings>("secret_settings");
}

export async function saveSecretCredential(
    input: SaveSecretCredentialInput,
): Promise<void> {
    return invoke<void>("save_secret_credential", {
        input,
    });
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

export async function connectRepositoryAuth(
    repositoryId: number,
    credentialsId: number,
): Promise<void> {
    return invoke<void>("connect_repository_auth", {
        input: {
            repository_id: repositoryId,
            credentials_id: credentialsId,
        },
    });
}

export async function reconnectRepositoryAuth(
    repositoryId: number,
    credentialsId: number,
): Promise<void> {
    return invoke<void>("reconnect_repository_auth", {
        input: {
            repository_id: repositoryId,
            credentials_id: credentialsId,
        },
    });
}

export async function disconnectRepositoryAuth(
    repositoryId: number,
): Promise<void> {
    return invoke<void>("disconnect_repository_auth", {
        input: {
            repository_id: repositoryId,
        },
    });
}

export async function syncRepositoryAuthAssessment(
    repositoryId: number,
    repositoryAccessAssessment: RepositoryAccessAssessment,
): Promise<void> {
    return invoke<void>("sync_repository_auth_assessment", {
        input: {
            repository_id: repositoryId,
            repository_access_assessment: repositoryAccessAssessment,
        },
    });
}