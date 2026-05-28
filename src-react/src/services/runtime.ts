import { invoke } from "@tauri-apps/api/core";

export type RuntimeHealthStatus =
    | "bootstrapping"
    | "healthy"
    | "shutting_down"
    | "stopped"
    | "unhealthy";

export type RuntimeHealthReport = {
    runtime_name: string;
    runtime_version: string;
    platform: string;
    log_level: string;
    status: RuntimeHealthStatus;
    process_id: number;
    started_at_unix: number;
    updated_at_unix: number;
    data_dir: string;
    database_path: string;
    health_report_path: string;
    log_file_path: string;
};

export type RuntimeAutomationMode = "active" | "idle";

export type RuntimeAutomationSnapshot = {
    mode: RuntimeAutomationMode;
    updated_at_unix: number;
};

export type ApplicationVersionInfo = {
    product_name: string;
    app_version: string;
};

export type RuntimeDirectorySettings = {
    data_dir: string;
    state_dir: string;
    logs_dir: string;
    artifacts_dir: string;
    runs_dir: string;
    database_path: string;
    health_report_path: string;
    supervision_contract_path: string;
    supervisor_state_path: string;
    runtime_events_path: string;
    runtime_events_cursor_path: string;
    runtime_log_path: string;
};

export type LocalizationLocaleSettings = {
    code: string;
    display_name: string;
    is_official: boolean;
    message_count: number;
    native_name: string;
};

export type LocalizationSettings = {
    available_locales: LocalizationLocaleSettings[];
    fallback_locale: string;
    localization_root: string;
    primary_locale: string;
    warnings: string[];
};

export type SaveLocalizationPreferencesInput = {
    fallback_locale: string;
    primary_locale: string;
};

type RepositoryInstantCheckInput = {
    repository_id: number;
};

export async function loadRuntimeHealth(): Promise<RuntimeHealthReport> {
    return invoke<RuntimeHealthReport>("runtime_health");
}

export async function loadApplicationVersion(): Promise<ApplicationVersionInfo> {
    return invoke<ApplicationVersionInfo>("application_version");
}

export async function loadRuntimeAutomationStatus(): Promise<RuntimeAutomationSnapshot> {
    return invoke<RuntimeAutomationSnapshot>("runtime_automation_status");
}

export async function loadRuntimeDirectories(): Promise<RuntimeDirectorySettings> {
    return invoke<RuntimeDirectorySettings>("runtime_directories");
}

export async function loadLocalizationSettings(): Promise<LocalizationSettings> {
    return invoke<LocalizationSettings>("localization_settings");
}

export async function startRuntime(): Promise<void> {
    return invoke<void>("start_runtime");
}

export async function stopRuntime(): Promise<void> {
    return invoke<void>("stop_runtime");
}

export async function restartRuntime(): Promise<void> {
    return invoke<void>("restart_runtime");
}

export async function setRuntimeAutomationMode(
    mode: RuntimeAutomationMode,
): Promise<RuntimeAutomationSnapshot> {
    return invoke<RuntimeAutomationSnapshot>("set_runtime_automation_mode", {
        mode,
    });
}

export async function saveLocalizationPreferences(
    input: SaveLocalizationPreferencesInput,
): Promise<LocalizationSettings> {
    return invoke<LocalizationSettings>("save_localization_preferences", {
        input,
    });
}

export async function requestRepositoryInstantCheck(
    repositoryId: number,
): Promise<void> {
    return invoke<void>("request_repository_instant_check", {
        input: {
            repository_id: repositoryId,
        } satisfies RepositoryInstantCheckInput,
    });
}

export async function openHostPath(path: string): Promise<void> {
    return invoke<void>("open_host_path", {
        path,
    });
}

export async function openExternalUrl(url: string): Promise<void> {
    return invoke<void>("open_external_url", {
        url,
    });
}