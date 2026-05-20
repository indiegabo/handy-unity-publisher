import { invoke } from "@tauri-apps/api/core";

export type JsonValue =
    | null
    | boolean
    | number
    | string
    | JsonValue[]
    | { [key: string]: JsonValue };

export type BuildHistoryRecord = {
    build_run_id: number;
    release_run_id: number;
    repository_id: number;
    repository_name: string;
    repository_url: string;
    git_tag: string;
    git_commit: string | null;
    build_target_id: number;
    build_target_name: string;
    unity_target_platform: string;
    runner_type: string;
    unity_build_method: string | null;
    engine_version: string | null;
    image_ref: string | null;
    status: string;
    workspace_path: string | null;
    log_path: string | null;
    artifact_root_path: string | null;
    started_at: string | null;
    finished_at: string | null;
    error_message: string | null;
    artifact_count: number;
    publish_run_count: number;
    created_at: string;
    updated_at: string;
};

export type ArtifactInspectionRecord = {
    artifact_id: number;
    build_run_id: number;
    release_run_id: number;
    repository_id: number;
    repository_name: string;
    repository_url: string;
    git_tag: string;
    git_commit: string | null;
    build_target_id: number;
    build_target_name: string;
    unity_target_platform: string;
    runner_type: string;
    build_status: string;
    artifact_name: string;
    artifact_kind: string;
    artifact_path: string;
    artifact_root_path: string | null;
    artifact_active_location_kind: string;
    artifact_active_location_ref: string;
    size_bytes: number | null;
    checksum_sha256: string | null;
    publish_run_count: number;
    queued_publish_runs: number;
    running_publish_runs: number;
    succeeded_publish_runs: number;
    failed_publish_runs: number;
    canceled_publish_runs: number;
    publish_runs: ArtifactPublishRunRecord[];
    created_at: string;
};

export type ArtifactPublishRunRecord = {
    publish_run_id: number;
    publish_target_id: number;
    publish_target_name: string;
    publish_target_kind: string;
    status: string;
    destination_ref: string | null;
    created_at: string;
    updated_at: string;
};

export type BuildExecutionReportPayload = {
    build_run_id: number;
    workspace_path: string | null;
    retained_dir_path: string | null;
    report_path: string | null;
    exists: boolean;
    logs_archive_path: string | null;
    logs_archive_exists: boolean;
    log_entries: RetainedLogArchiveEntry[];
    report: JsonValue | null;
};

export type RetainedLogArchiveEntry = {
    entry_path: string;
    entry_name: string;
    size_bytes: number;
    compressed_size_bytes: number;
};

export type HostTextFilePayload = {
    path: string;
    exists: boolean;
    size_bytes: number;
    truncated: boolean;
    content: string;
};

export type ReleaseProcessOutputsDeleteReport = {
    release_run_id: number;
    artifact_root_path: string | null;
    removed_paths: string[];
    missing_paths: string[];
};

export type BuildLogDeleteReport = {
    build_run_id: number;
    log_path: string | null;
    removed_paths: string[];
    missing_paths: string[];
    parent_removed: boolean;
};

export type BuildExecutionRetentionPurgeReport = {
    build_run_id: number;
    workspace_path: string | null;
    retained_dir_path: string | null;
    removed_paths: string[];
    workspace_removed: boolean;
};

export type RetainedLogArchiveEntryPreviewPayload = {
    archive_path: string;
    entry_path: string;
    exists: boolean;
    size_bytes: number;
    truncated: boolean;
    content: string;
};

export async function loadBuildHistory(): Promise<BuildHistoryRecord[]> {
    const payload = await invoke<unknown[]>("build_history");
    return Array.isArray(payload) ? payload.map(normalizeBuildHistoryRecord) : [];
}

export async function loadArtifactInspection(): Promise<ArtifactInspectionRecord[]> {
    const payload = await invoke<unknown[]>("artifact_inspection");
    return Array.isArray(payload)
        ? payload.map(normalizeArtifactInspectionRecord)
        : [];
}

export async function loadBuildExecutionReport(
    buildRunId: number,
): Promise<BuildExecutionReportPayload> {
    const payload = await invoke<unknown>("build_execution_report", {
        buildRunId,
    });
    return normalizeBuildExecutionReportPayload(payload);
}

export async function readRetainedLogArchiveEntry(
    buildRunId: number,
    entryPath: string,
    maxBytes?: number,
): Promise<RetainedLogArchiveEntryPreviewPayload> {
    const payload = await invoke<unknown>("read_retained_log_archive_entry", {
        buildRunId,
        entryPath,
        maxBytes,
    });
    return normalizeRetainedLogArchiveEntryPreviewPayload(payload);
}

export async function purgeBuildExecutionRetention(
    buildRunId: number,
): Promise<BuildExecutionRetentionPurgeReport> {
    const payload = await invoke<unknown>("purge_build_execution_retention", {
        buildRunId,
    });
    return normalizeBuildExecutionRetentionPurgeReport(payload);
}

export async function openHostPath(path: string): Promise<void> {
    return invoke<void>("open_host_path", { path });
}

export async function readHostTextFile(
    path: string,
    maxBytes?: number,
): Promise<HostTextFilePayload> {
    const payload = await invoke<unknown>("read_host_text_file", {
        path,
        maxBytes,
    });
    return normalizeHostTextFilePayload(payload);
}

export async function deleteReleaseProcessOutputs(
    releaseRunId: number,
): Promise<ReleaseProcessOutputsDeleteReport> {
    const payload = await invoke<unknown>(
        "delete_release_process_outputs",
        {
            releaseRunId,
        },
    );
    return normalizeReleaseProcessOutputsDeleteReport(payload);
}

export async function rerunReleaseProcess(
    releaseRunId: number,
): Promise<void> {
    return invoke<void>("rerun_release_process", {
        releaseRunId,
    });
}

export async function deleteBuildLog(
    buildRunId: number,
): Promise<BuildLogDeleteReport> {
    const payload = await invoke<unknown>("delete_build_log", {
        buildRunId,
    });
    return normalizeBuildLogDeleteReport(payload);
}

function normalizeBuildHistoryRecord(value: unknown): BuildHistoryRecord {
    const record = asRecord(value);

    return {
        build_run_id: readNumber(record, "build_run_id", "buildRunId"),
        release_run_id: readNumber(record, "release_run_id", "releaseRunId"),
        repository_id: readNumber(record, "repository_id", "repositoryId"),
        repository_name: readString(record, "repository_name", "repositoryName"),
        repository_url: readString(record, "repository_url", "repositoryUrl"),
        git_tag: readString(record, "git_tag", "gitTag"),
        git_commit: readNullableString(record, "git_commit", "gitCommit"),
        build_target_id: readNumber(record, "build_target_id", "buildTargetId"),
        build_target_name: readString(record, "build_target_name", "buildTargetName"),
        unity_target_platform: readString(
            record,
            "unity_target_platform",
            "unityTargetPlatform",
        ),
        runner_type: readString(record, "runner_type", "runnerType"),
        unity_build_method: readNullableString(
            record,
            "unity_build_method",
            "unityBuildMethod",
        ),
        engine_version: readNullableString(record, "engine_version", "engineVersion"),
        image_ref: readNullableString(record, "image_ref", "imageRef"),
        status: readString(record, "status"),
        workspace_path: readNullableString(record, "workspace_path", "workspacePath"),
        log_path: readNullableString(record, "log_path", "logPath"),
        artifact_root_path: readNullableString(
            record,
            "artifact_root_path",
            "artifactRootPath",
        ),
        started_at: readNullableString(record, "started_at", "startedAt"),
        finished_at: readNullableString(record, "finished_at", "finishedAt"),
        error_message: readNullableString(record, "error_message", "errorMessage"),
        artifact_count: readNumber(record, "artifact_count", "artifactCount"),
        publish_run_count: readNumber(record, "publish_run_count", "publishRunCount"),
        created_at: readString(record, "created_at", "createdAt"),
        updated_at: readString(record, "updated_at", "updatedAt"),
    };
}

function normalizeArtifactInspectionRecord(value: unknown): ArtifactInspectionRecord {
    const record = asRecord(value);

    return {
        artifact_id: readNumber(record, "artifact_id", "artifactId"),
        build_run_id: readNumber(record, "build_run_id", "buildRunId"),
        release_run_id: readNumber(record, "release_run_id", "releaseRunId"),
        repository_id: readNumber(record, "repository_id", "repositoryId"),
        repository_name: readString(record, "repository_name", "repositoryName"),
        repository_url: readString(record, "repository_url", "repositoryUrl"),
        git_tag: readString(record, "git_tag", "gitTag"),
        git_commit: readNullableString(record, "git_commit", "gitCommit"),
        build_target_id: readNumber(record, "build_target_id", "buildTargetId"),
        build_target_name: readString(record, "build_target_name", "buildTargetName"),
        unity_target_platform: readString(
            record,
            "unity_target_platform",
            "unityTargetPlatform",
        ),
        runner_type: readString(record, "runner_type", "runnerType"),
        build_status: readString(record, "build_status", "buildStatus"),
        artifact_name: readString(record, "artifact_name", "artifactName"),
        artifact_kind: readString(record, "artifact_kind", "artifactKind"),
        artifact_path: readString(record, "artifact_path", "artifactPath"),
        artifact_root_path: readNullableString(
            record,
            "artifact_root_path",
            "artifactRootPath",
        ),
        artifact_active_location_kind: readString(
            record,
            "artifact_active_location_kind",
            "artifactActiveLocationKind",
        ),
        artifact_active_location_ref: readString(
            record,
            "artifact_active_location_ref",
            "artifactActiveLocationRef",
        ),
        size_bytes: readNullableNumber(record, "size_bytes", "sizeBytes"),
        checksum_sha256: readNullableString(
            record,
            "checksum_sha256",
            "checksumSha256",
        ),
        publish_run_count: readNumber(record, "publish_run_count", "publishRunCount"),
        queued_publish_runs: readNumber(
            record,
            "queued_publish_runs",
            "queuedPublishRuns",
        ),
        running_publish_runs: readNumber(
            record,
            "running_publish_runs",
            "runningPublishRuns",
        ),
        succeeded_publish_runs: readNumber(
            record,
            "succeeded_publish_runs",
            "succeededPublishRuns",
        ),
        failed_publish_runs: readNumber(
            record,
            "failed_publish_runs",
            "failedPublishRuns",
        ),
        canceled_publish_runs: readNumber(
            record,
            "canceled_publish_runs",
            "canceledPublishRuns",
        ),
        publish_runs: readArray(record, "publish_runs", "publishRuns").map(
            normalizeArtifactPublishRunRecord,
        ),
        created_at: readString(record, "created_at", "createdAt"),
    };
}

function normalizeArtifactPublishRunRecord(value: unknown): ArtifactPublishRunRecord {
    const record = asRecord(value);

    return {
        publish_run_id: readNumber(record, "publish_run_id", "publishRunId"),
        publish_target_id: readNumber(
            record,
            "publish_target_id",
            "publishTargetId",
        ),
        publish_target_name: readString(
            record,
            "publish_target_name",
            "publishTargetName",
        ),
        publish_target_kind: readString(
            record,
            "publish_target_kind",
            "publishTargetKind",
        ),
        status: readString(record, "status"),
        destination_ref: readNullableString(
            record,
            "destination_ref",
            "destinationRef",
        ),
        created_at: readString(record, "created_at", "createdAt"),
        updated_at: readString(record, "updated_at", "updatedAt"),
    };
}

function normalizeBuildExecutionReportPayload(value: unknown): BuildExecutionReportPayload {
    const record = asRecord(value);

    return {
        build_run_id: readNumber(record, "build_run_id", "buildRunId"),
        workspace_path: readNullableString(record, "workspace_path", "workspacePath"),
        retained_dir_path: readNullableString(
            record,
            "retained_dir_path",
            "retainedDirPath",
        ),
        report_path: readNullableString(record, "report_path", "reportPath"),
        exists: readBoolean(record, "exists"),
        logs_archive_path: readNullableString(
            record,
            "logs_archive_path",
            "logsArchivePath",
        ),
        logs_archive_exists: readBoolean(
            record,
            "logs_archive_exists",
            "logsArchiveExists",
        ),
        log_entries: readRetainedLogArchiveEntries(record, "log_entries", "logEntries"),
        report: readJsonValue(record, "report"),
    };
}

function normalizeRetainedLogArchiveEntryPreviewPayload(
    value: unknown,
): RetainedLogArchiveEntryPreviewPayload {
    const record = asRecord(value);

    return {
        archive_path: readString(record, "archive_path", "archivePath"),
        entry_path: readString(record, "entry_path", "entryPath"),
        exists: readBoolean(record, "exists"),
        size_bytes: readNumber(record, "size_bytes", "sizeBytes"),
        truncated: readBoolean(record, "truncated"),
        content: readString(record, "content"),
    };
}

function normalizeHostTextFilePayload(value: unknown): HostTextFilePayload {
    const record = asRecord(value);

    return {
        path: readString(record, "path"),
        exists: readBoolean(record, "exists"),
        size_bytes: readNumber(record, "size_bytes", "sizeBytes"),
        truncated: readBoolean(record, "truncated"),
        content: readString(record, "content"),
    };
}

function normalizeReleaseProcessOutputsDeleteReport(
    value: unknown,
): ReleaseProcessOutputsDeleteReport {
    const record = asRecord(value);

    return {
        release_run_id: readNumber(record, "release_run_id", "releaseRunId"),
        artifact_root_path: readNullableString(
            record,
            "artifact_root_path",
            "artifactRootPath",
        ),
        removed_paths: readStringArray(record, "removed_paths", "removedPaths"),
        missing_paths: readStringArray(record, "missing_paths", "missingPaths"),
    };
}

function normalizeBuildLogDeleteReport(value: unknown): BuildLogDeleteReport {
    const record = asRecord(value);

    return {
        build_run_id: readNumber(record, "build_run_id", "buildRunId"),
        log_path: readNullableString(record, "log_path", "logPath"),
        removed_paths: readStringArray(record, "removed_paths", "removedPaths"),
        missing_paths: readStringArray(record, "missing_paths", "missingPaths"),
        parent_removed: readBoolean(record, "parent_removed", "parentRemoved"),
    };
}

function normalizeBuildExecutionRetentionPurgeReport(
    value: unknown,
): BuildExecutionRetentionPurgeReport {
    const record = asRecord(value);

    return {
        build_run_id: readNumber(record, "build_run_id", "buildRunId"),
        workspace_path: readNullableString(record, "workspace_path", "workspacePath"),
        retained_dir_path: readNullableString(
            record,
            "retained_dir_path",
            "retainedDirPath",
        ),
        removed_paths: readStringArray(record, "removed_paths", "removedPaths"),
        workspace_removed: readBoolean(record, "workspace_removed", "workspaceRemoved"),
    };
}

function readRetainedLogArchiveEntries(
    record: Record<string, unknown>,
    ...keys: string[]
) {
    for (const key of keys) {
        const value = record[key];
        if (Array.isArray(value)) {
            return value.map(normalizeRetainedLogArchiveEntry);
        }
    }

    return [];
}

function normalizeRetainedLogArchiveEntry(value: unknown): RetainedLogArchiveEntry {
    const record = asRecord(value);

    return {
        entry_path: readString(record, "entry_path", "entryPath"),
        entry_name: readString(record, "entry_name", "entryName"),
        size_bytes: readNumber(record, "size_bytes", "sizeBytes"),
        compressed_size_bytes: readNumber(
            record,
            "compressed_size_bytes",
            "compressedSizeBytes",
        ),
    };
}

function asRecord(value: unknown): Record<string, unknown> {
    if (value && typeof value === "object" && !Array.isArray(value)) {
        return value as Record<string, unknown>;
    }

    return {};
}

function readString(record: Record<string, unknown>, ...keys: string[]) {
    for (const key of keys) {
        const value = record[key];
        if (typeof value === "string") {
            return value;
        }
    }

    return "";
}

function readNullableString(record: Record<string, unknown>, ...keys: string[]) {
    for (const key of keys) {
        const value = record[key];
        if (typeof value === "string") {
            return value;
        }
        if (value === null) {
            return null;
        }
    }

    return null;
}

function readNumber(record: Record<string, unknown>, ...keys: string[]) {
    for (const key of keys) {
        const value = record[key];
        if (typeof value === "number" && Number.isFinite(value)) {
            return value;
        }
    }

    return 0;
}

function readNullableNumber(record: Record<string, unknown>, ...keys: string[]) {
    for (const key of keys) {
        const value = record[key];
        if (typeof value === "number" && Number.isFinite(value)) {
            return value;
        }
        if (value === null) {
            return null;
        }
    }

    return null;
}

function readBoolean(record: Record<string, unknown>, ...keys: string[]) {
    for (const key of keys) {
        const value = record[key];
        if (typeof value === "boolean") {
            return value;
        }
    }

    return false;
}

function readStringArray(record: Record<string, unknown>, ...keys: string[]) {
    for (const key of keys) {
        const value = record[key];
        if (Array.isArray(value)) {
            return value.filter((entry): entry is string => typeof entry === "string");
        }
    }

    return [];
}

function readArray(record: Record<string, unknown>, ...keys: string[]) {
    for (const key of keys) {
        const value = record[key];
        if (Array.isArray(value)) {
            return value;
        }
    }

    return [] as unknown[];
}

function readJsonValue(record: Record<string, unknown>, ...keys: string[]): JsonValue | null {
    for (const key of keys) {
        const value = record[key];
        if (isJsonValue(value)) {
            return value;
        }
        if (value === null) {
            return null;
        }
    }

    return null;
}

function isJsonValue(value: unknown): value is JsonValue {
    if (
        value === null ||
        typeof value === "boolean" ||
        typeof value === "number" ||
        typeof value === "string"
    ) {
        return true;
    }

    if (Array.isArray(value)) {
        return value.every(isJsonValue);
    }

    if (typeof value !== "object") {
        return false;
    }

    return Object.values(value as Record<string, unknown>).every(isJsonValue);
}
