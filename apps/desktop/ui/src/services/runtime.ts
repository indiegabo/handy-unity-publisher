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

type RepositoryInstantCheckInput = {
    repository_id: number;
};

export async function loadRuntimeHealth(): Promise<RuntimeHealthReport> {
    return invoke<RuntimeHealthReport>("runtime_health");
}

export async function loadRuntimeAutomationStatus(): Promise<RuntimeAutomationSnapshot> {
    return invoke<RuntimeAutomationSnapshot>("runtime_automation_status");
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

export async function requestRepositoryInstantCheck(
    repositoryId: number,
): Promise<void> {
    return invoke<void>("request_repository_instant_check", {
        input: {
            repository_id: repositoryId,
        } satisfies RepositoryInstantCheckInput,
    });
}