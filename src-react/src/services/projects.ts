import { invoke } from "@tauri-apps/api/core";

export type UnityExecutableValidation = {
    runner_family: string;
    unity_executable_path: string | null;
    process_priority?: BuildProcessPriority | null;
    unity_executable_exists: boolean;
    unity_executable_is_file: boolean;
    additional_argument_count: number;
    environment_variable_count: number;
    status: string;
    message: string;
};

export type ProcessPriority = "low" | "normal" | "high";

export type BuildProcessPriority = ProcessPriority;

export type DiscoveredUnityEditor = {
    version: string;
    source: string;
    install_root_path: string;
    executable_path: string;
    executable_exists: boolean;
    executable_is_file: boolean;
    supported_build_targets: string[];
    status: string;
    message: string;
};

export type HostCapabilityProfile = {
    discovered_editors: DiscoveredUnityEditor[];
};

export type UnityAdapterSettings = {
    capability_profile: HostCapabilityProfile;
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
    source_mode?: string | null;
    workspace_strategy?: string | null;
    repo_url: string;
    local_path?: string | null;
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
    process_priority: BuildProcessPriority;
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
    source_mode: "managed_repository" | "local_workspace";
    repository_url?: string | null;
    local_path?: string | null;
    repository_access_assessment?: RepositoryAccessAssessment | null;
    repository_credentials_id?: number | null;
    default_branch?: string | null;
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
    process_priority: BuildProcessPriority;
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
    source_mode: "managed_repository" | "local_workspace";
    repository_url?: string | null;
    local_path?: string | null;
    repository_access_assessment?: RepositoryAccessAssessment | null;
    default_branch?: string | null;
    workspace_root_override?: string | null;
    polling_interval_seconds: number;
    enabled: boolean;
    build_targets: UpdateRepositoryProjectBuildTargetInput[];
    publish_targets: UpdateRepositoryProjectPublishTargetInput[];
};

export type RemoveRepositoryProjectStrategy = "detach" | "purge";

export type RemoveRepositoryProjectInput = {
    repository_id: number;
    strategy: RemoveRepositoryProjectStrategy;
};

export type RemoveRepositoryProjectReport = {
    repository_id: number;
    repository_name: string;
    strategy: RemoveRepositoryProjectStrategy;
    release_run_count: number;
    build_run_count: number;
    publish_run_count: number;
    queue_message_count: number;
    coordination_lease_count: number;
    idempotency_key_count: number;
    removed_paths: string[];
    missing_paths: string[];
    skipped_paths: string[];
};

export type OnDemandReleaseVersionSource =
    | "manual"
    | "project_settings"
    | "source_tag";

export type OnDemandReleaseSourceKind =
    | "managed_tag"
    | "managed_ref"
    | "local_workspace";

export type OnDemandReleaseProcessInput = {
    repository_id: number;
    release_version?: string | null;
    version_source: OnDemandReleaseVersionSource;
    source_kind: OnDemandReleaseSourceKind;
    source_ref?: string | null;
    local_path?: string | null;
    process_priority?: ProcessPriority | null;
    unity_executable_path_override?: string | null;
};

export type OnDemandReleaseVersionPreviewInput = {
    repository_id: number;
    version_source: OnDemandReleaseVersionSource;
    source_kind: OnDemandReleaseSourceKind;
    source_ref?: string | null;
    local_path?: string | null;
};

export type OnDemandReleaseRemoteRefsInput = {
    repository_id: number;
    source_kind: Extract<OnDemandReleaseSourceKind, "managed_ref" | "managed_tag">;
};

export type OnDemandReleaseRemoteRef = {
    name: string;
    commit: string;
};

export type QueuedReleaseRunRecord = {
    id: number;
    repository_id: number;
    git_tag: string;
    status: string;
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
): Promise<number> {
    return invoke<number>("save_secret_credential", {
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

export async function loadDefaultProjectWorkspaceRoot(
    projectName?: string | null,
): Promise<string> {
    return invoke<string>("default_project_workspace_root", {
        projectName: projectName?.trim() || null,
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

export async function loadUnityAdapterSettings(): Promise<UnityAdapterSettings> {
    return invoke<UnityAdapterSettings>("unity_adapter_settings");
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

export async function removeRepositoryProject(
    input: RemoveRepositoryProjectInput,
): Promise<RemoveRepositoryProjectReport> {
    return invoke<RemoveRepositoryProjectReport>("remove_repository_project", {
        input,
    });
}

export async function dispatchOnDemandReleaseProcess(
    input: OnDemandReleaseProcessInput,
): Promise<QueuedReleaseRunRecord> {
    return invoke<QueuedReleaseRunRecord>("dispatch_on_demand_release_process", {
        input,
    });
}

export async function previewOnDemandReleaseVersion(
    input: OnDemandReleaseVersionPreviewInput,
): Promise<string> {
    return invoke<string>("preview_on_demand_release_version", {
        input,
    });
}

export async function listOnDemandReleaseRemoteRefs(
    input: OnDemandReleaseRemoteRefsInput,
): Promise<OnDemandReleaseRemoteRef[]> {
    return invoke<OnDemandReleaseRemoteRef[]>("list_on_demand_release_remote_refs", {
        input,
    });
}

/** Reads the Unity `bundleVersion` from `ProjectSettings/ProjectSettings.asset`
 * inside the given local workspace path. Throws with a descriptive message on
 * IO or parse errors. */
export async function readProjectSettingsVersion(
    localPath: string,
): Promise<string> {
    return invoke<string>("read_project_settings_version", { localPath });
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