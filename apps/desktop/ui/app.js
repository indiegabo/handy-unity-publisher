const COMMANDS = {
    runtimeHealth: "runtime_health",
    runtimeLogs: "runtime_logs",
    runtimeDirectories: "runtime_directories",
    runtimeLifecycleSettings: "runtime_lifecycle_settings",
    releaseStatus: "release_status",
    repositoryInspection: "repository_inspection",
    buildHistory: "build_history",
    artifactInspection: "artifact_inspection",
    secretSettings: "secret_settings",
    saveSecretCredential: "save_secret_credential",
    updateRepositorySecretBinding: "update_repository_secret_binding",
    updatePublishTargetSecretBinding: "update_publish_target_secret_binding",
    unityRunnerSettings: "unity_runner_settings",
};

const SECTION_LABELS = {
    artifacts: "Artifact inspection",
    builds: "Build history",
    health: "Runtime health",
    repositories: "Repository inspection",
    releases: "Release status",
    logs: "Recent logs",
    directories: "Runtime directories",
    lifecycle: "Lifecycle rules",
    secrets: "Secret settings",
    unity: "Unity runners",
};

const SUMMARY_ORDER = [
    "health",
    "repositories",
    "builds",
    "artifacts",
    "releases",
    "lifecycle",
    "unity",
    "secrets",
    "logs",
];
const DEFAULT_REFRESH_INTERVAL_MILLIS = 15000;
const DEFAULT_LOG_LINE_LIMIT = 100;
const DEFAULT_LOG_LEVEL_FILTER = "all";
const UI_PREFERENCES_STORAGE_KEY = "desktop-diagnostics-preferences";

const uiState = {
    autoRefreshEnabled: true,
    refreshIntervalMillis: DEFAULT_REFRESH_INTERVAL_MILLIS,
    logLineLimit: DEFAULT_LOG_LINE_LIMIT,
    logLevelFilter: DEFAULT_LOG_LEVEL_FILTER,
    logSearchQuery: "",
    secretEditorCredentialId: null,
    secretEditorName: "",
    secretEditorKind: "",
    secretEditorConfigJson: "",
    secretActionStatus: null,
    secretActionInFlight: false,
    lastDiagnostics: null,
    refreshTimerId: null,
    refreshInFlight: false,
    refreshQueued: false,
};

document.addEventListener("DOMContentLoaded", () => {
    restoreUiPreferences();
    syncUiControls();
    bindControls();
    renderStaticLoadingState();
    updatePollingStateMessage();
    void refreshDiagnostics("startup");
});

function bindControls() {
    document.getElementById("refresh-button").addEventListener("click", () => {
        void refreshDiagnostics("manual");
    });

    document
        .getElementById("auto-refresh-toggle")
        .addEventListener("change", (event) => {
            uiState.autoRefreshEnabled = event.target.checked;
            persistUiPreferences();
            scheduleAutoRefresh();
        });

    document
        .getElementById("refresh-interval-select")
        .addEventListener("change", (event) => {
            const nextValue = Number(event.target.value);
            uiState.refreshIntervalMillis =
                Number.isFinite(nextValue) && nextValue > 0
                    ? nextValue
                    : DEFAULT_REFRESH_INTERVAL_MILLIS;
            persistUiPreferences();
            scheduleAutoRefresh();
        });

    document
        .getElementById("log-line-limit-select")
        .addEventListener("change", (event) => {
            const nextValue = Number(event.target.value);
            uiState.logLineLimit =
                Number.isFinite(nextValue) && nextValue > 0
                    ? nextValue
                    : DEFAULT_LOG_LINE_LIMIT;
            persistUiPreferences();
            queueRefresh("log-window");
        });

    document
        .getElementById("log-level-filter-select")
        .addEventListener("change", (event) => {
            uiState.logLevelFilter = event.target.value;
            persistUiPreferences();
            rerenderLogsOnly();
        });

    document
        .getElementById("log-search-input")
        .addEventListener("input", (event) => {
            uiState.logSearchQuery = event.target.value;
            persistUiPreferences();
            rerenderLogsOnly();
        });

    document.addEventListener("visibilitychange", handleVisibilityChange);
    window.addEventListener("beforeunload", clearScheduledRefresh);
}

function restoreUiPreferences() {
    const preferences = readStoredUiPreferences();

    uiState.autoRefreshEnabled = preferences.autoRefreshEnabled;
    uiState.refreshIntervalMillis = preferences.refreshIntervalMillis;
    uiState.logLineLimit = preferences.logLineLimit;
    uiState.logLevelFilter = preferences.logLevelFilter;
    uiState.logSearchQuery = preferences.logSearchQuery;
}

function readStoredUiPreferences() {
    const defaults = {
        autoRefreshEnabled: true,
        refreshIntervalMillis: DEFAULT_REFRESH_INTERVAL_MILLIS,
        logLineLimit: DEFAULT_LOG_LINE_LIMIT,
        logLevelFilter: DEFAULT_LOG_LEVEL_FILTER,
        logSearchQuery: "",
    };

    try {
        const raw = window.localStorage.getItem(UI_PREFERENCES_STORAGE_KEY);
        if (!raw) {
            return defaults;
        }

        const parsed = JSON.parse(raw);
        return {
            autoRefreshEnabled:
                typeof parsed?.autoRefreshEnabled === "boolean"
                    ? parsed.autoRefreshEnabled
                    : defaults.autoRefreshEnabled,
            refreshIntervalMillis: normalizePositiveInteger(
                parsed?.refreshIntervalMillis,
                defaults.refreshIntervalMillis,
            ),
            logLineLimit: normalizePositiveInteger(
                parsed?.logLineLimit,
                defaults.logLineLimit,
            ),
            logLevelFilter: normalizeLogLevelFilter(
                parsed?.logLevelFilter,
                defaults.logLevelFilter,
            ),
            logSearchQuery:
                typeof parsed?.logSearchQuery === "string"
                    ? parsed.logSearchQuery
                    : defaults.logSearchQuery,
        };
    } catch {
        return defaults;
    }
}

function persistUiPreferences() {
    try {
        window.localStorage.setItem(
            UI_PREFERENCES_STORAGE_KEY,
            JSON.stringify({
                autoRefreshEnabled: uiState.autoRefreshEnabled,
                refreshIntervalMillis: uiState.refreshIntervalMillis,
                logLineLimit: uiState.logLineLimit,
                logLevelFilter: uiState.logLevelFilter,
                logSearchQuery: uiState.logSearchQuery,
            }),
        );
    } catch {
        // Ignore storage errors and keep the shell usable.
    }
}

function syncUiControls() {
    document.getElementById("auto-refresh-toggle").checked =
        uiState.autoRefreshEnabled;
    document.getElementById("refresh-interval-select").value = String(
        uiState.refreshIntervalMillis,
    );
    document.getElementById("log-line-limit-select").value = String(
        uiState.logLineLimit,
    );
    document.getElementById("log-level-filter-select").value =
        uiState.logLevelFilter;
    document.getElementById("log-search-input").value = uiState.logSearchQuery;
}

function normalizePositiveInteger(value, fallback) {
    const parsed = Number(value);
    if (!Number.isInteger(parsed) || parsed <= 0) {
        return fallback;
    }

    return parsed;
}

function normalizeLogLevelFilter(value, fallback) {
    if (typeof value !== "string") {
        return fallback;
    }

    return ["all", "info", "warn", "error"].includes(value)
        ? value
        : fallback;
}

function handleVisibilityChange() {
    if (document.hidden) {
        clearScheduledRefresh();
        updatePollingStateMessage();
        return;
    }

    if (uiState.autoRefreshEnabled) {
        void refreshDiagnostics("resume");
        return;
    }

    updatePollingStateMessage();
}

async function refreshDiagnostics(reason = "manual") {
    if (uiState.refreshInFlight) {
        uiState.refreshQueued = true;
        updatePollingStateMessage();
        return;
    }

    clearScheduledRefresh();
    uiState.refreshInFlight = true;
    setConnectionState(connectionPendingMessage(reason), "pending");
    setRefreshButtonState(true);
    updatePollingStateMessage();

    try {
        const diagnostics = await loadDiagnostics();
        uiState.lastDiagnostics = diagnostics;
        renderDiagnostics(diagnostics);
    } finally {
        uiState.refreshInFlight = false;
        setRefreshButtonState(false);

        if (uiState.refreshQueued) {
            uiState.refreshQueued = false;
            void refreshDiagnostics("queued");
            return;
        }

        scheduleAutoRefresh();
    }
}

function queueRefresh(reason = "manual") {
    if (uiState.refreshInFlight) {
        uiState.refreshQueued = true;
        updatePollingStateMessage();
        return;
    }

    void refreshDiagnostics(reason);
}

function scheduleAutoRefresh() {
    clearScheduledRefresh();
    updatePollingStateMessage();

    if (!uiState.autoRefreshEnabled) {
        return;
    }

    if (document.hidden) {
        return;
    }

    if (uiState.lastDiagnostics?.connected === false) {
        return;
    }

    uiState.refreshTimerId = window.setTimeout(() => {
        void refreshDiagnostics("poll");
    }, uiState.refreshIntervalMillis);
}

function clearScheduledRefresh() {
    if (uiState.refreshTimerId === null) {
        return;
    }

    window.clearTimeout(uiState.refreshTimerId);
    uiState.refreshTimerId = null;
}

async function loadDiagnostics() {
    const invoker = getTauriInvoker();
    if (!invoker) {
        return offlineDiagnostics();
    }

    const results = await Promise.allSettled([
        invoker(COMMANDS.runtimeHealth),
        invoker(COMMANDS.runtimeLogs, { lineLimit: uiState.logLineLimit }),
        invoker(COMMANDS.runtimeDirectories),
        invoker(COMMANDS.runtimeLifecycleSettings),
        invoker(COMMANDS.releaseStatus),
        invoker(COMMANDS.repositoryInspection),
        invoker(COMMANDS.buildHistory),
        invoker(COMMANDS.artifactInspection),
        invoker(COMMANDS.secretSettings),
        invoker(COMMANDS.unityRunnerSettings),
    ]);

    return {
        health: settleResult(results[0]),
        logs: settleResult(results[1]),
        directories: settleResult(results[2]),
        lifecycle: settleResult(results[3]),
        releaseStatus: settleResult(results[4]),
        repositoryInspection: settleResult(results[5]),
        buildHistory: settleResult(results[6]),
        artifactInspection: settleResult(results[7]),
        secrets: settleResult(results[8]),
        unity: settleResult(results[9]),
        loadedAt: new Date(),
        connected: true,
    };
}

function getTauriInvoker() {
    const invoke = window.__TAURI__?.core?.invoke;
    return typeof invoke === "function" ? invoke : null;
}

function offlineDiagnostics() {
    const offlineError = {
        ok: false,
        error: {
            title: "Tauri API unavailable",
            detail:
                "window.__TAURI__.core.invoke is not available. Open this page inside the desktop shell to fetch live runtime diagnostics.",
        },
    };

    return {
        health: offlineError,
        logs: offlineError,
        directories: offlineError,
        lifecycle: offlineError,
        releaseStatus: offlineError,
        repositoryInspection: offlineError,
        buildHistory: offlineError,
        artifactInspection: offlineError,
        secrets: offlineError,
        unity: offlineError,
        loadedAt: new Date(),
        connected: false,
    };
}

function settleResult(result) {
    if (result.status === "fulfilled") {
        return { ok: true, data: result.value };
    }

    return {
        ok: false,
        error: {
            title: normalizeErrorTitle(result.reason),
            detail: normalizeErrorDetail(result.reason),
        },
    };
}

function normalizeErrorTitle(reason) {
    if (typeof reason === "string" && reason.trim()) {
        return reason.trim();
    }

    if (reason && typeof reason === "object" && "message" in reason) {
        return String(reason.message);
    }

    return "Runtime command failed";
}

function normalizeErrorDetail(reason) {
    if (typeof reason === "string" && reason.trim()) {
        return reason.trim();
    }

    if (reason && typeof reason === "object") {
        try {
            return JSON.stringify(reason, null, 2);
        } catch {
            return String(reason);
        }
    }

    return "The shell could not decode the command error payload.";
}

function renderStaticLoadingState() {
    renderSummaryCards([]);
    setPill("artifact-inspection-pill", "Waiting", "neutral");
    setPill("build-history-pill", "Waiting", "neutral");
    setPill("repository-inspection-pill", "Waiting", "neutral");
    setPill("release-status-pill", "Waiting", "neutral");
    setPill("logs-pill", "Waiting", "neutral");
    document.getElementById("logs-meta").textContent =
        "Waiting for runtime log stream.";
    renderPanelBody("health-panel", loadingMarkup("Waiting for runtime health."));
    renderPanelBody(
        "lifecycle-panel",
        loadingMarkup("Waiting for lifecycle policy metadata."),
    );
    renderPanelBody(
        "directories-panel",
        loadingMarkup("Waiting for runtime filesystem paths."),
    );
    renderPanelBody(
        "release-status-panel",
        loadingMarkup("Waiting for release automation snapshot."),
    );
    renderPanelBody(
        "repository-inspection-panel",
        loadingMarkup("Waiting for repository inspection metadata."),
    );
    renderPanelBody(
        "build-history-panel",
        loadingMarkup("Waiting for persisted build history."),
    );
    renderPanelBody(
        "artifact-inspection-panel",
        loadingMarkup("Waiting for persisted artifact metadata."),
    );
    renderPanelBody(
        "unity-panel",
        loadingMarkup("Waiting for Unity runner state."),
    );
    renderPanelBody(
        "secret-panel",
        loadingMarkup("Waiting for secret settings."),
    );
    renderPanelBody(
        "logs-panel",
        loadingMarkup("Waiting for runtime log stream."),
    );
}

function renderDiagnostics(diagnostics) {
    setConnectionState(
        diagnostics.connected
            ? "Desktop shell connected to runtime commands"
            : "Diagnostics fallback mode",
        diagnostics.connected ? "ready" : "warning",
    );
    document.getElementById("last-refresh").textContent = `Last refresh ${formatDateTime(
        diagnostics.loadedAt,
    )}`;

    renderSummaryCards(buildSummaryCards(diagnostics));
    renderHealth(diagnostics.health);
    renderLifecycle(diagnostics.lifecycle);
    renderReleaseStatus(diagnostics.releaseStatus);
    renderRepositoryInspection(diagnostics.repositoryInspection);
    renderBuildHistory(diagnostics.buildHistory);
    renderArtifactInspection(diagnostics.artifactInspection);
    renderDirectories(diagnostics.directories);
    renderUnity(diagnostics.unity);
    renderSecrets(diagnostics.secrets);
    renderLogs(diagnostics.logs);
    updatePollingStateMessage();
}

function buildSummaryCards(diagnostics) {
    const cards = [];

    if (diagnostics.health.ok) {
        cards.push({
            key: "health",
            label: "Runtime health",
            value: diagnostics.health.data.status,
            tone: toneForStatus(diagnostics.health.data.status),
            detail: diagnostics.health.data.message,
        });
    } else {
        cards.push(errorSummaryCard("health", diagnostics.health.error.title));
    }

    if (diagnostics.lifecycle.ok) {
        cards.push({
            key: "lifecycle",
            label: "Crash recovery",
            value: diagnostics.lifecycle.data.crash_recovery_status,
            tone: toneForStatus(diagnostics.lifecycle.data.crash_recovery_status),
            detail: `Restart budget ${diagnostics.lifecycle.data.restart_policy.max_restarts}`,
        });
    } else {
        cards.push(errorSummaryCard("lifecycle", diagnostics.lifecycle.error.title));
    }

    if (diagnostics.repositoryInspection.ok) {
        const repositorySummary = summarizeRepositoryInspection(
            diagnostics.repositoryInspection.data,
        );
        cards.push({
            key: "repositories",
            label: "Repositories",
            value: buildRepositoryInspectionLabel(repositorySummary),
            tone: toneForRepositoryInspectionSummary(repositorySummary),
            detail: buildRepositoryInspectionSummaryDetail(repositorySummary),
        });
    } else {
        cards.push(
            errorSummaryCard(
                "repositories",
                diagnostics.repositoryInspection.error.title,
            ),
        );
    }

    if (diagnostics.buildHistory.ok) {
        const buildSummary = summarizeBuildHistory(diagnostics.buildHistory.data);
        cards.push({
            key: "builds",
            label: "Build history",
            value: buildBuildHistoryLabel(buildSummary),
            tone: toneForBuildHistorySummary(buildSummary),
            detail: buildBuildHistorySummaryDetail(buildSummary),
        });
    } else {
        cards.push(errorSummaryCard("builds", diagnostics.buildHistory.error.title));
    }

    if (diagnostics.artifactInspection.ok) {
        const artifactSummary = summarizeArtifactInspection(
            diagnostics.artifactInspection.data,
        );
        cards.push({
            key: "artifacts",
            label: "Artifacts",
            value: buildArtifactInspectionLabel(artifactSummary),
            tone: toneForArtifactInspectionSummary(artifactSummary),
            detail: buildArtifactInspectionSummaryDetail(artifactSummary),
        });
    } else {
        cards.push(
            errorSummaryCard(
                "artifacts",
                diagnostics.artifactInspection.error.title,
            ),
        );
    }

    if (diagnostics.releaseStatus.ok) {
        const releaseSummary = summarizeReleaseSnapshot(
            diagnostics.releaseStatus.data,
        );
        cards.push({
            key: "releases",
            label: "Release queue",
            value:
                releaseSummary.pendingReleaseCount > 0
                    ? `${releaseSummary.pendingReleaseCount} pending`
                    : "Idle",
            tone: toneForReleaseSummary(releaseSummary),
            detail: buildReleaseSummaryDetail(releaseSummary),
        });
    } else {
        cards.push(errorSummaryCard("releases", diagnostics.releaseStatus.error.title));
    }

    if (diagnostics.unity.ok) {
        const readyTargets = diagnostics.unity.data.build_targets.filter(
            (target) => target.diagnostic_status === "ready",
        ).length;
        cards.push({
            key: "unity",
            label: "Unity targets",
            value: `${readyTargets}/${diagnostics.unity.data.build_targets.length} ready`,
            tone: readyTargets > 0 ? "ready" : "warning",
            detail: diagnostics.unity.data.supported_runner_families.join(", "),
        });
    } else {
        cards.push(errorSummaryCard("unity", diagnostics.unity.error.title));
    }

    if (diagnostics.secrets.ok) {
        cards.push({
            key: "secrets",
            label: "Credential entries",
            value: String(diagnostics.secrets.data.credentials.length),
            tone: diagnostics.secrets.data.warnings.length > 0 ? "warning" : "ready",
            detail: diagnostics.secrets.data.storage_model,
        });
    } else {
        cards.push(errorSummaryCard("secrets", diagnostics.secrets.error.title));
    }

    if (diagnostics.logs.ok) {
        cards.push({
            key: "logs",
            label: "Recent log lines",
            value: String(diagnostics.logs.data.length),
            tone: diagnostics.logs.data.length > 0 ? "ready" : "neutral",
            detail: "Most recent JSONL events from runtime.jsonl",
        });
    } else {
        cards.push(errorSummaryCard("logs", diagnostics.logs.error.title));
    }

    cards.sort(
        (left, right) =>
            SUMMARY_ORDER.indexOf(left.key) - SUMMARY_ORDER.indexOf(right.key),
    );
    return cards;
}

function errorSummaryCard(key, detail) {
    return {
        key,
        label: SECTION_LABELS[key],
        value: "Unavailable",
        tone: "error",
        detail,
    };
}

function renderSummaryCards(cards) {
    const root = document.getElementById("summary-grid");
    if (cards.length === 0) {
        root.innerHTML = "";
        return;
    }

    root.innerHTML = cards
        .map(
            (card) => `
                <article class="summary-card tone-${escapeHtml(card.tone)}">
                    <p class="summary-label">${escapeHtml(card.label)}</p>
                    <strong class="summary-value">${escapeHtml(card.value)}</strong>
                    <p class="summary-detail">${escapeHtml(card.detail)}</p>
                </article>
            `,
        )
        .join("");
}

function renderBuildHistory(result) {
    if (!result.ok) {
        setPill("build-history-pill", "Error", "error");
        renderPanelBody("build-history-panel", errorMarkup(result.error));
        return;
    }

    const builds = result.data;
    const summary = summarizeBuildHistory(builds);
    const buildsMarkup = builds.length
        ? builds.map(renderBuildHistoryEntry).join("")
        : '<div class="callout tone-neutral"><strong>Build history</strong><p>No persisted build runs were found yet.</p></div>';

    setPill(
        "build-history-pill",
        buildBuildHistoryLabel(summary),
        toneForBuildHistorySummary(summary),
    );
    renderPanelBody(
        "build-history-panel",
        `
            <div class="callout tone-${escapeHtml(
            toneForBuildHistorySummary(summary),
        )}">
                <strong>${escapeHtml(buildBuildHistoryHeadline(summary))}</strong>
                <p>${escapeHtml(buildBuildHistoryOverview(summary))}</p>
            </div>
            ${keyValueGrid([
            ["Recorded builds", String(summary.totalCount)],
            ["Queued", String(summary.queuedCount)],
            ["Running", String(summary.runningCount)],
            ["Succeeded", String(summary.succeededCount)],
            ["Failed", String(summary.failedCount)],
            ["Canceled", String(summary.canceledCount)],
            ["Artifacts", String(summary.artifactCount)],
            ["Publish fan-out", String(summary.publishRunCount)],
        ])}
            <section>
                <h3>Recorded builds</h3>
                <div class="subpanel-grid">${buildsMarkup}</div>
            </section>
        `,
    );
}

function summarizeBuildHistory(builds) {
    return builds.reduce(
        (summary, build) => {
            summary.totalCount += 1;
            summary.artifactCount += build.artifact_count;
            summary.publishRunCount += build.publish_run_count;

            switch (build.status) {
                case "queued":
                    summary.queuedCount += 1;
                    break;
                case "running":
                    summary.runningCount += 1;
                    break;
                case "succeeded":
                    summary.succeededCount += 1;
                    break;
                case "failed":
                    summary.failedCount += 1;
                    break;
                case "canceled":
                    summary.canceledCount += 1;
                    break;
                default:
                    break;
            }

            return summary;
        },
        {
            totalCount: 0,
            queuedCount: 0,
            runningCount: 0,
            succeededCount: 0,
            failedCount: 0,
            canceledCount: 0,
            artifactCount: 0,
            publishRunCount: 0,
        },
    );
}

function buildBuildHistoryLabel(summary) {
    if (summary.totalCount === 0) {
        return "0 builds";
    }

    if (summary.runningCount > 0) {
        return `${summary.runningCount} running`;
    }

    if (summary.failedCount > 0) {
        return `${summary.failedCount} failed`;
    }

    return `${summary.totalCount} builds`;
}

function toneForBuildHistorySummary(summary) {
    if (summary.failedCount > 0) {
        return "error";
    }

    if (summary.runningCount > 0 || summary.succeededCount > 0) {
        return "ready";
    }

    if (summary.queuedCount > 0 || summary.canceledCount > 0) {
        return "warning";
    }

    return "neutral";
}

function buildBuildHistorySummaryDetail(summary) {
    if (summary.totalCount === 0) {
        return "No persisted build runs have been recorded yet.";
    }

    return `${summary.succeededCount} succeeded, ${summary.runningCount} running, ${summary.failedCount} failed, ${summary.canceledCount} canceled, ${summary.artifactCount} artifacts, ${summary.publishRunCount} publish runs.`;
}

function buildBuildHistoryHeadline(summary) {
    if (summary.totalCount === 0) {
        return "The runtime has not recorded build execution yet";
    }

    if (summary.failedCount > 0) {
        return "Recent build history includes failed runs";
    }

    if (summary.runningCount > 0) {
        return "Build workers are actively moving local work";
    }

    if (summary.queuedCount > 0) {
        return "Queued builds are waiting for the next worker claim";
    }

    return "Recent build history is fully persisted";
}

function buildBuildHistoryOverview(summary) {
    if (summary.totalCount === 0) {
        return "The desktop shell has not found persisted build runs in the runtime database yet.";
    }

    return `${summary.totalCount} recorded ${pluralize(summary.totalCount, "build")}, ${summary.artifactCount} discovered ${pluralize(summary.artifactCount, "artifact")}, and ${summary.publishRunCount} downstream publish ${pluralize(summary.publishRunCount, "run")}.`;
}

function renderBuildHistoryEntry(build) {
    const pathMarkup = [
        build.workspace_path
            ? `<li class="compact-item"><span class="pill neutral">workspace</span><code>${escapeHtml(build.workspace_path)}</code></li>`
            : "",
        build.log_path
            ? `<li class="compact-item"><span class="pill neutral">log</span><code>${escapeHtml(build.log_path)}</code></li>`
            : "",
        build.artifact_root_path
            ? `<li class="compact-item"><span class="pill neutral">artifacts</span><code>${escapeHtml(build.artifact_root_path)}</code></li>`
            : "",
    ]
        .filter(Boolean)
        .join("");
    const errorMarkup = build.error_message
        ? `
                <div class="callout tone-error">
                    <strong>Failure detail</strong>
                    <p>${escapeHtml(build.error_message)}</p>
                </div>
            `
        : "";

    return `
        <article class="subpanel">
            <div class="subpanel-header">
                <strong>
                    ${escapeHtml(build.repository_name)} /
                    ${escapeHtml(build.build_target_name)}
                </strong>
                <span class="pill ${escapeHtml(toneForStatus(build.status))}">
                    ${escapeHtml(build.status)}
                </span>
            </div>
            ${keyValueGrid([
        ["Build id", String(build.build_run_id)],
        ["Release", `${build.git_tag} (#${build.release_run_id})`],
        ["Unity", build.unity_version || "not resolved"],
        ["Runner", build.runner_type],
        ["Build method", build.build_method || "not configured"],
        ["Artifacts", String(build.artifact_count)],
        ["Publish fan-out", String(build.publish_run_count)],
        ["Started", optionalDateTime(build.started_at)],
        ["Finished", optionalDateTime(build.finished_at)],
    ])}
            <div class="callout tone-neutral">
                <strong>Source release</strong>
                <p>${escapeHtml(build.repository_url)}</p>
                <p class="muted">Git commit: ${escapeHtml(build.git_commit || "not recorded")}</p>
            </div>
            ${pathMarkup ? `<ul class="compact-list">${pathMarkup}</ul>` : ""}
            ${errorMarkup}
        </article>
    `;
}

function renderArtifactInspection(result) {
    if (!result.ok) {
        setPill("artifact-inspection-pill", "Error", "error");
        renderPanelBody("artifact-inspection-panel", errorMarkup(result.error));
        return;
    }

    const artifacts = result.data;
    const summary = summarizeArtifactInspection(artifacts);
    const artifactsMarkup = artifacts.length
        ? artifacts.map(renderArtifactInspectionEntry).join("")
        : '<div class="callout tone-neutral"><strong>Artifact inspection</strong><p>No persisted artifacts were found yet.</p></div>';

    setPill(
        "artifact-inspection-pill",
        buildArtifactInspectionLabel(summary),
        toneForArtifactInspectionSummary(summary),
    );
    renderPanelBody(
        "artifact-inspection-panel",
        `
            <div class="callout tone-${escapeHtml(
            toneForArtifactInspectionSummary(summary),
        )}">
                <strong>${escapeHtml(buildArtifactInspectionHeadline(summary))}</strong>
                <p>${escapeHtml(buildArtifactInspectionOverview(summary))}</p>
            </div>
            ${keyValueGrid([
            ["Recorded artifacts", String(summary.totalCount)],
            ["Repositories", String(summary.repositoryCount)],
            ["Published", String(summary.publishedArtifactCount)],
            ["Active publishes", String(summary.activeArtifactCount)],
            ["Unpublished", String(summary.unpublishedArtifactCount)],
            ["Publish runs", String(summary.publishRunCount)],
            ["Failed publishes", String(summary.failedPublishRuns)],
            ["Known size", formatByteSize(summary.totalSizeBytes)],
        ])}
            <section>
                <h3>Persisted artifacts</h3>
                <div class="subpanel-grid">${artifactsMarkup}</div>
            </section>
        `,
    );
}

function summarizeArtifactInspection(artifacts) {
    const summary = {
        totalCount: 0,
        repositoryCount: 0,
        publishRunCount: 0,
        queuedPublishRuns: 0,
        runningPublishRuns: 0,
        succeededPublishRuns: 0,
        failedPublishRuns: 0,
        canceledPublishRuns: 0,
        publishedArtifactCount: 0,
        activeArtifactCount: 0,
        unpublishedArtifactCount: 0,
        totalSizeBytes: 0,
        sizedArtifactCount: 0,
    };
    const repositories = new Set();

    for (const artifact of artifacts) {
        summary.totalCount += 1;
        summary.publishRunCount += artifact.publish_run_count;
        summary.queuedPublishRuns += artifact.queued_publish_runs;
        summary.runningPublishRuns += artifact.running_publish_runs;
        summary.succeededPublishRuns += artifact.succeeded_publish_runs;
        summary.failedPublishRuns += artifact.failed_publish_runs;
        summary.canceledPublishRuns += artifact.canceled_publish_runs;
        repositories.add(artifact.repository_id);

        if (artifact.succeeded_publish_runs > 0) {
            summary.publishedArtifactCount += 1;
        }

        if (
            artifact.queued_publish_runs > 0 ||
            artifact.running_publish_runs > 0
        ) {
            summary.activeArtifactCount += 1;
        }

        if (artifact.publish_run_count === 0) {
            summary.unpublishedArtifactCount += 1;
        }

        if (typeof artifact.size_bytes === "number" && artifact.size_bytes >= 0) {
            summary.totalSizeBytes += artifact.size_bytes;
            summary.sizedArtifactCount += 1;
        }
    }

    summary.repositoryCount = repositories.size;
    return summary;
}

function buildArtifactInspectionLabel(summary) {
    if (summary.totalCount === 0) {
        return "0 artifacts";
    }

    if (summary.failedPublishRuns > 0) {
        return `${summary.failedPublishRuns} publish failures`;
    }

    if (summary.runningPublishRuns > 0) {
        return `${summary.runningPublishRuns} publishing`;
    }

    if (summary.queuedPublishRuns > 0) {
        return `${summary.queuedPublishRuns} queued`;
    }

    return `${summary.totalCount} artifacts`;
}

function toneForArtifactInspectionSummary(summary) {
    if (summary.failedPublishRuns > 0) {
        return "error";
    }

    if (summary.runningPublishRuns > 0 || summary.queuedPublishRuns > 0) {
        return "warning";
    }

    if (summary.totalCount > 0) {
        return "ready";
    }

    return "neutral";
}

function buildArtifactInspectionSummaryDetail(summary) {
    if (summary.totalCount === 0) {
        return "No persisted artifacts have been registered yet.";
    }

    const sizeDetail =
        summary.sizedArtifactCount > 0
            ? ` ${formatByteSize(summary.totalSizeBytes)} across ${summary.sizedArtifactCount} sized ${pluralize(summary.sizedArtifactCount, "artifact")}.`
            : "";

    return `${summary.publishedArtifactCount} published, ${summary.activeArtifactCount} with active publish fan-out, ${summary.unpublishedArtifactCount} without downstream publishes.${sizeDetail}`;
}

function buildArtifactInspectionHeadline(summary) {
    if (summary.totalCount === 0) {
        return "The runtime has not registered artifacts yet";
    }

    if (summary.failedPublishRuns > 0) {
        return "Some persisted artifacts have failed publish fan-out";
    }

    if (summary.runningPublishRuns > 0 || summary.queuedPublishRuns > 0) {
        return "Persisted artifacts are still moving through publish workers";
    }

    if (summary.unpublishedArtifactCount === summary.totalCount) {
        return "Artifacts are registered without downstream publish runs";
    }

    return "Persisted artifacts are ready for inspection";
}

function buildArtifactInspectionOverview(summary) {
    if (summary.totalCount === 0) {
        return "The desktop shell has not found persisted artifact rows in the runtime database yet.";
    }

    const sizeDetail =
        summary.sizedArtifactCount > 0
            ? ` ${formatByteSize(summary.totalSizeBytes)} total payload.`
            : "";

    return `${summary.totalCount} persisted ${pluralize(summary.totalCount, "artifact")} across ${summary.repositoryCount} ${pluralize(summary.repositoryCount, "repository")} and ${summary.publishRunCount} downstream publish ${pluralize(summary.publishRunCount, "run")}.${sizeDetail}`;
}

function renderArtifactInspectionEntry(artifact) {
    const pathMarkup = [
        artifact.artifact_root_path
            ? `<li class="compact-item"><span class="pill neutral">root</span><code>${escapeHtml(artifact.artifact_root_path)}</code></li>`
            : "",
        artifact.artifact_path
            ? `<li class="compact-item"><span class="pill neutral">artifact</span><code>${escapeHtml(artifact.artifact_path)}</code></li>`
            : "",
    ]
        .filter(Boolean)
        .join("");

    return `
        <article class="subpanel">
            <div class="subpanel-header">
                <strong>
                    ${escapeHtml(artifact.repository_name)} /
                    ${escapeHtml(artifact.artifact_name)}
                </strong>
                <span class="pill ${escapeHtml(toneForArtifactPublishState(artifact))}">
                    ${escapeHtml(buildArtifactPublishLabel(artifact))}
                </span>
            </div>
            ${keyValueGrid([
        ["Artifact id", String(artifact.artifact_id)],
        ["Build id", String(artifact.build_run_id)],
        ["Release", `${artifact.git_tag} (#${artifact.release_run_id})`],
        ["Target", artifact.build_target_name],
        ["Kind", artifact.artifact_kind],
        ["Platform", artifact.platform],
        ["Runner", artifact.runner_type],
        ["Build status", artifact.build_status],
        ["Size", formatByteSize(artifact.size_bytes)],
        ["Created", optionalDateTime(artifact.created_at)],
    ])}
            ${keyValueGrid([
        ["Publish runs", String(artifact.publish_run_count)],
        ["Queued publishes", String(artifact.queued_publish_runs)],
        ["Running publishes", String(artifact.running_publish_runs)],
        ["Succeeded publishes", String(artifact.succeeded_publish_runs)],
        ["Failed publishes", String(artifact.failed_publish_runs)],
        ["Canceled publishes", String(artifact.canceled_publish_runs)],
        ["Checksum", artifact.checksum_sha256 || "not recorded"],
    ])}
            <div class="callout tone-neutral">
                <strong>Source release</strong>
                <p>${escapeHtml(artifact.repository_url)}</p>
                <p class="muted">Git commit: ${escapeHtml(artifact.git_commit || "not recorded")}</p>
            </div>
            ${pathMarkup ? `<ul class="compact-list">${pathMarkup}</ul>` : ""}
        </article>
    `;
}

function buildArtifactPublishLabel(artifact) {
    if (artifact.publish_run_count === 0) {
        return "unpublished";
    }

    if (artifact.failed_publish_runs > 0) {
        return `${artifact.failed_publish_runs} failed`;
    }

    if (artifact.running_publish_runs > 0) {
        return `${artifact.running_publish_runs} running`;
    }

    if (artifact.queued_publish_runs > 0) {
        return `${artifact.queued_publish_runs} queued`;
    }

    if (artifact.succeeded_publish_runs > 0) {
        return `${artifact.succeeded_publish_runs} published`;
    }

    return `${artifact.publish_run_count} publishes`;
}

function toneForArtifactPublishState(artifact) {
    if (artifact.failed_publish_runs > 0) {
        return "error";
    }

    if (artifact.running_publish_runs > 0 || artifact.queued_publish_runs > 0) {
        return "warning";
    }

    if (artifact.succeeded_publish_runs > 0) {
        return "ready";
    }

    return "neutral";
}

function renderHealth(result) {
    if (!result.ok) {
        setPill("health-pill", "Error", "error");
        renderPanelBody("health-panel", errorMarkup(result.error));
        return;
    }

    const health = result.data;
    setPill("health-pill", health.status, toneForStatus(health.status));
    renderPanelBody(
        "health-panel",
        `
            ${keyValueGrid([
            ["Status", health.status],
            ["Runtime", health.runtime_name],
            ["Version", health.runtime_version],
            ["Process id", optionalNumber(health.process_id)],
            ["Updated", formatUnixSeconds(health.updated_at_unix)],
        ])}
            <div class="callout tone-${escapeHtml(toneForStatus(health.status))}">
                <strong>Latest message</strong>
                <p>${escapeHtml(health.message)}</p>
            </div>
        `,
    );
}

function renderLifecycle(result) {
    if (!result.ok) {
        setPill("lifecycle-pill", "Error", "error");
        renderPanelBody("lifecycle-panel", errorMarkup(result.error));
        return;
    }

    const lifecycle = result.data;
    setPill(
        "lifecycle-pill",
        lifecycle.crash_recovery_status,
        toneForStatus(lifecycle.crash_recovery_status),
    );
    renderPanelBody(
        "lifecycle-panel",
        `
            ${keyValueGrid([
            ["Startup action", commandLabel(lifecycle.startup_command)],
            ["Shutdown action", commandLabel(lifecycle.shutdown_command)],
            ["Grace period", `${lifecycle.shutdown_grace_period_millis} ms`],
            ["Max restarts", String(lifecycle.restart_policy.max_restarts)],
            [
                "Backoff",
                `${lifecycle.restart_policy.restart_backoff_millis} ms`,
            ],
            [
                "Recoverable exit code",
                String(lifecycle.restart_policy.recoverable_exit_code),
            ],
        ])}
            ${booleanChecklist([
            [
                "Shell launches supervisor on startup",
                lifecycle.shell_launches_supervisor_on_startup,
            ],
            [
                "Shell requests shutdown on exit",
                lifecycle.shell_requests_shutdown_on_exit,
            ],
            [
                "Shell force kills after timeout",
                lifecycle.shell_force_kills_after_timeout,
            ],
            [
                "Shell relaunches on desktop restart",
                lifecycle.shell_relaunches_supervisor_on_restart,
            ],
            [
                "Runtime supervisor owns crash recovery",
                lifecycle.runtime_supervisor_owns_crash_recovery,
            ],
        ])}
            ${renderSupervisorSnapshot(lifecycle.supervisor_snapshot)}
        `,
    );
}

function renderSupervisorSnapshot(snapshot) {
    if (!snapshot) {
        return '<div class="callout tone-neutral"><strong>Supervisor snapshot</strong><p>No persisted snapshot was found yet.</p></div>';
    }

    return `
        <div class="callout tone-${escapeHtml(toneForStatus(snapshot.status))}">
            <strong>Supervisor snapshot</strong>
            <p>${escapeHtml(snapshot.message)}</p>
            ${keyValueGrid([
        ["Status", snapshot.status],
        ["Restart count", String(snapshot.restart_count)],
        ["Attempt count", String(snapshot.attempt_count)],
        ["Supervisor pid", optionalNumber(snapshot.supervisor_process_id)],
        ["Runtime pid", optionalNumber(snapshot.runtime_process_id)],
        ["Last exit code", optionalNumber(snapshot.last_exit_code)],
    ])}
        </div>
    `;
}

function renderReleaseStatus(result) {
    if (!result.ok) {
        setPill("release-status-pill", "Error", "error");
        renderPanelBody("release-status-panel", errorMarkup(result.error));
        return;
    }

    const snapshot = result.data;
    const summary = summarizeReleaseSnapshot(snapshot);
    const activeRepositories = snapshot.repositories.filter(
        (repository) => repository.pending_release_count > 0,
    );
    const queueMarkup = snapshot.queue_messages.length
        ? `
                <ul class="compact-list">
                    ${snapshot.queue_messages
            .map(
                (queue) => `
                                <li class="compact-item">
                                    <span class="pill ${escapeHtml(toneForQueueSnapshot(queue))}">
                                        ${escapeHtml(`${queue.ready_count} ready`)}
                                    </span>
                                    <span class="pill neutral">
                                        ${escapeHtml(`${queue.leased_count} leased`)}
                                    </span>
                                    <code>${escapeHtml(queue.queue_name)}</code>
                                </li>
                            `,
            )
            .join("")}
                </ul>
            `
        : '<div class="callout tone-neutral"><strong>Worker queues</strong><p>No persisted queue pressure is visible yet.</p></div>';
    const leaseMarkup = snapshot.coordination_leases.length
        ? `
                <ul class="compact-list">
                    ${snapshot.coordination_leases
            .map(
                (lease) => `
                                <li class="compact-item">
                                    <span class="pill warning">lease</span>
                                    <code>${escapeHtml(lease.name)}</code>
                                    <span>${escapeHtml(
                    `expires ${formatUnixMilliseconds(lease.lease_expires_at_unix_millis)}`,
                )}</span>
                                </li>
                            `,
            )
            .join("")}
                </ul>
            `
        : '<div class="callout tone-neutral"><strong>Coordination leases</strong><p>No active coordination lease is blocking a repository lane.</p></div>';
    const repositoryMarkup = activeRepositories.length
        ? activeRepositories.map(renderReleaseRepository).join("")
        : '<div class="callout tone-neutral"><strong>Repository lanes</strong><p>No repository currently has a queued or running release backlog.</p></div>';

    setPill(
        "release-status-pill",
        buildReleaseSummaryLabel(summary),
        toneForReleaseSummary(summary),
    );
    renderPanelBody(
        "release-status-panel",
        `
            <div class="callout tone-${escapeHtml(toneForReleaseSummary(summary))}">
                <strong>${escapeHtml(buildReleaseHeadline(summary))}</strong>
                <p>${escapeHtml(buildReleaseOverview(summary))}</p>
            </div>
            ${keyValueGrid([
            [
                "Snapshot",
                snapshot.generated_at
                    ? formatDateTime(snapshot.generated_at)
                    : "not yet sampled",
            ],
            ["Tracked repositories", String(summary.trackedRepositoryCount)],
            ["Repositories with backlog", String(summary.activeRepositoryCount)],
            ["Pending releases", String(summary.pendingReleaseCount)],
            ["Queued builds", String(summary.queuedBuildRuns)],
            ["Running builds", String(summary.runningBuildRuns)],
            ["Queued publishes", String(summary.queuedPublishRuns)],
            ["Running publishes", String(summary.runningPublishRuns)],
        ])}
            <section>
                <h3>Worker queues</h3>
                ${queueMarkup}
            </section>
            <section>
                <h3>Coordination leases</h3>
                ${leaseMarkup}
            </section>
            <section>
                <h3>Repositories with active releases</h3>
                <div class="subpanel-grid">${repositoryMarkup}</div>
            </section>
        `,
    );
}

function renderRepositoryInspection(result) {
    if (!result.ok) {
        setPill("repository-inspection-pill", "Error", "error");
        renderPanelBody("repository-inspection-panel", errorMarkup(result.error));
        return;
    }

    const inspection = result.data;
    const summary = summarizeRepositoryInspection(inspection);
    const repositoryMarkup = inspection.repositories.length
        ? inspection.repositories.map(renderInspectedRepository).join("")
        : '<div class="callout tone-neutral"><strong>Repository definitions</strong><p>No repository definitions have been persisted yet.</p></div>';

    setPill(
        "repository-inspection-pill",
        buildRepositoryInspectionLabel(summary),
        toneForRepositoryInspectionSummary(summary),
    );
    renderPanelBody(
        "repository-inspection-panel",
        `
            <div class="callout tone-${escapeHtml(
            toneForRepositoryInspectionSummary(summary),
        )}">
                <strong>${escapeHtml(
            buildRepositoryInspectionHeadline(summary),
        )}</strong>
                <p>${escapeHtml(buildRepositoryInspectionOverview(summary))}</p>
            </div>
            ${keyValueGrid([
            [
                "Snapshot",
                inspection.generated_at
                    ? formatDateTime(inspection.generated_at)
                    : "not yet sampled",
            ],
            ["Tracked repositories", String(summary.totalCount)],
            ["Enabled repositories", String(summary.enabledCount)],
            [
                "Repositories with backlog",
                String(summary.backlogRepositoryCount),
            ],
            ["Build targets", String(summary.buildTargetCount)],
            ["Publish targets", String(summary.publishTargetCount)],
            [
                "Repositories without build targets",
                String(summary.targetlessCount),
            ],
        ])}
            <section>
                <h3>Repository definitions</h3>
                <div class="subpanel-grid">${repositoryMarkup}</div>
            </section>
        `,
    );
}

function summarizeRepositoryInspection(inspection) {
    return inspection.repositories.reduce(
        (summary, repository) => {
            summary.totalCount += 1;
            summary.enabledCount += repository.enabled ? 1 : 0;
            summary.backlogRepositoryCount +=
                repository.pending_release_count > 0 ? 1 : 0;
            summary.targetlessCount += repository.build_targets.length === 0 ? 1 : 0;
            summary.buildTargetCount += repository.build_targets.length;
            summary.publishTargetCount += repository.publish_targets.length;
            return summary;
        },
        {
            totalCount: 0,
            enabledCount: 0,
            backlogRepositoryCount: 0,
            targetlessCount: 0,
            buildTargetCount: 0,
            publishTargetCount: 0,
        },
    );
}

function buildRepositoryInspectionLabel(summary) {
    if (summary.totalCount === 0) {
        return "0 repos";
    }

    return `${summary.enabledCount}/${summary.totalCount} enabled`;
}

function buildRepositoryInspectionSummaryDetail(summary) {
    if (summary.totalCount === 0) {
        return "No repository definitions have been persisted yet.";
    }

    const parts = [
        `${summary.buildTargetCount} build ${pluralize(summary.buildTargetCount, "target")}`,
        `${summary.publishTargetCount} publish ${pluralize(summary.publishTargetCount, "target")}`,
    ];

    if (summary.backlogRepositoryCount > 0) {
        parts.push(
            `${summary.backlogRepositoryCount} with active ${pluralize(summary.backlogRepositoryCount, "backlog")}`,
        );
    }

    if (summary.targetlessCount > 0) {
        parts.push(
            `${summary.targetlessCount} without build ${pluralize(summary.targetlessCount, "target")}`,
        );
    }

    return parts.join(", ") + ".";
}

function toneForRepositoryInspectionSummary(summary) {
    if (summary.totalCount === 0) {
        return "neutral";
    }

    if (summary.backlogRepositoryCount > 0 || summary.targetlessCount > 0) {
        return "warning";
    }

    if (summary.enabledCount > 0) {
        return "ready";
    }

    return "neutral";
}

function buildRepositoryInspectionHeadline(summary) {
    if (summary.totalCount === 0) {
        return "No repository definitions are loaded";
    }

    if (summary.backlogRepositoryCount > 0) {
        return "Repository lanes are carrying active local work";
    }

    if (summary.targetlessCount > 0) {
        return "Some repository definitions still lack build targets";
    }

    return "Repository definitions are aligned with the local runtime";
}

function buildRepositoryInspectionOverview(summary) {
    if (summary.totalCount === 0) {
        return "The desktop shell has not found persisted repository pipeline definitions in the runtime database yet.";
    }

    return `${summary.totalCount} tracked ${pluralize(summary.totalCount, "repository")}, ${summary.enabledCount} enabled for polling, ${summary.buildTargetCount} build ${pluralize(summary.buildTargetCount, "target")}, and ${summary.publishTargetCount} publish ${pluralize(summary.publishTargetCount, "target")}.`;
}

function renderInspectedRepository(repository) {
    const buildTargetsMarkup = repository.build_targets.length
        ? repository.build_targets.map(renderRepositoryBuildTarget).join("")
        : '<div class="callout tone-neutral"><strong>Build targets</strong><p>No build targets are attached to this repository yet.</p></div>';
    const publishTargetsMarkup = repository.publish_targets.length
        ? repository.publish_targets.map(renderRepositoryPublishTarget).join("")
        : '<div class="callout tone-neutral"><strong>Publish targets</strong><p>No publish targets are attached to this repository yet.</p></div>';
    const releaseQueueMarkup = repository.release_queue.length
        ? `
                <ul class="compact-list">
                    ${repository.release_queue
            .map(renderRepositoryReleaseQueueItem)
            .join("")}
                </ul>
            `
        : '<div class="callout tone-neutral"><strong>Release lane</strong><p>No queued release currently blocks this repository.</p></div>';

    return `
        <article class="subpanel">
            <div class="subpanel-header">
                <strong>${escapeHtml(repository.repository_name)}</strong>
                <span class="pill ${escapeHtml(
        toneForInspectedRepository(repository),
    )}">
                    ${escapeHtml(buildInspectedRepositoryLabel(repository))}
                </span>
            </div>
            ${keyValueGrid([
        ["Repository id", String(repository.repository_id)],
        ["Git remote", repository.repo_url],
        ["Enabled", repository.enabled ? "yes" : "no"],
        ["Poll interval", `${repository.polling_interval_seconds} s`],
        ["Last seen tag", repository.last_seen_tag || "not recorded"],
        ["Enabled targets", String(repository.enabled_build_target_count)],
        ["Pending releases", String(repository.pending_release_count)],
    ])}
            ${renderRepositoryCredentialCallout(
        "Repository credentials",
        repository.credentials,
        "No repository credential is bound. Public Git access or another external credential path must satisfy this remote.",
    )}
            <section>
                <h3>Build targets</h3>
                <div class="subpanel-grid">${buildTargetsMarkup}</div>
            </section>
            <section>
                <h3>Publish targets</h3>
                <div class="subpanel-grid">${publishTargetsMarkup}</div>
            </section>
            <section>
                <h3>Pending release lane</h3>
                ${releaseQueueMarkup}
            </section>
        </article>
    `;
}

function toneForInspectedRepository(repository) {
    if (!repository.enabled) {
        return "neutral";
    }

    if (
        repository.running_build_runs > 0 ||
        repository.running_publish_runs > 0
    ) {
        return "ready";
    }

    if (repository.pending_release_count > 0) {
        return "warning";
    }

    return "ready";
}

function buildInspectedRepositoryLabel(repository) {
    if (!repository.enabled) {
        return "disabled";
    }

    if (repository.pending_release_count > 0) {
        return `${repository.pending_release_count} pending`;
    }

    return "enabled";
}

function renderRepositoryCredentialCallout(title, credential, emptyMessage) {
    if (!credential) {
        return `
            <div class="callout tone-neutral">
                <strong>${escapeHtml(title)}</strong>
                <p>${escapeHtml(emptyMessage)}</p>
            </div>
        `;
    }

    return `
        <div class="callout tone-${escapeHtml(
        toneForStatus(credential.config_status),
    )}">
            <strong>${escapeHtml(title)}</strong>
            <p>${escapeHtml(
        `${credential.name} (${credential.kind})`,
    )}</p>
            <p class="muted">${escapeHtml(credential.config_message)}</p>
        </div>
    `;
}

function renderRepositoryBuildTarget(target) {
    return `
        <article class="subpanel">
            <div class="subpanel-header">
                <strong>${escapeHtml(target.target_name)}</strong>
                <span class="pill ${escapeHtml(
        toneForStatus(target.diagnostic_status),
    )}">
                    ${escapeHtml(target.diagnostic_status)}
                </span>
            </div>
            ${keyValueGrid([
        ["Platform", target.platform],
        ["Runner", target.runner_type],
        ["Build method", target.build_method || "not configured"],
        ["Enabled", target.enabled ? "yes" : "no"],
    ])}
            <p class="subpanel-copy">${escapeHtml(target.diagnostic_message)}</p>
        </article>
    `;
}

function renderRepositoryPublishTarget(target) {
    const credentialsLabel = target.credentials
        ? `${target.credentials.name} (${target.credentials.kind})`
        : "not bound";
    const configStatus = target.credentials
        ? target.credentials.config_status
        : "not_bound";
    const diagnosticCopy = target.credentials
        ? target.credentials.config_message
        : "No credential is bound to this publish target.";

    return `
        <article class="subpanel">
            <div class="subpanel-header">
                <strong>${escapeHtml(target.name)}</strong>
                <span class="pill ${target.enabled ? "ready" : "neutral"}">
                    ${escapeHtml(target.kind)}
                </span>
            </div>
            ${keyValueGrid([
        ["Kind", target.kind],
        ["Enabled", target.enabled ? "yes" : "no"],
        ["Credentials", credentialsLabel],
        ["Config status", configStatus],
    ])}
            <p class="subpanel-copy">${escapeHtml(diagnosticCopy)}</p>
        </article>
    `;
}

function renderRepositoryReleaseQueueItem(release) {
    return `
        <li class="compact-item">
            <span class="pill ${escapeHtml(toneForReleaseQueueItem(release))}">
                ${escapeHtml(release.status)}
            </span>
            <strong>${escapeHtml(release.git_tag)}</strong>
            <span>${escapeHtml(buildReleaseQueueSummary(release))}</span>
        </li>
    `;
}

function summarizeReleaseSnapshot(snapshot) {
    return snapshot.repositories.reduce(
        (summary, repository) => {
            summary.trackedRepositoryCount += 1;
            summary.activeRepositoryCount +=
                repository.pending_release_count > 0 ? 1 : 0;
            summary.pendingReleaseCount += repository.pending_release_count;
            summary.queuedBuildRuns += repository.queued_build_runs;
            summary.runningBuildRuns += repository.running_build_runs;
            summary.queuedPublishRuns += repository.queued_publish_runs;
            summary.runningPublishRuns += repository.running_publish_runs;
            return summary;
        },
        {
            trackedRepositoryCount: 0,
            activeRepositoryCount: 0,
            pendingReleaseCount: 0,
            queuedBuildRuns: 0,
            runningBuildRuns: 0,
            queuedPublishRuns: 0,
            runningPublishRuns: 0,
            readyQueueMessages: snapshot.queue_messages.reduce(
                (count, queue) => count + queue.ready_count,
                0,
            ),
            leasedQueueMessages: snapshot.queue_messages.reduce(
                (count, queue) => count + queue.leased_count,
                0,
            ),
            coordinationLeaseCount: snapshot.coordination_leases.length,
        },
    );
}

function buildReleaseSummaryLabel(summary) {
    return summary.pendingReleaseCount > 0
        ? `${summary.pendingReleaseCount} pending`
        : "Idle";
}

function buildReleaseSummaryDetail(summary) {
    if (summary.pendingReleaseCount === 0) {
        if (summary.trackedRepositoryCount === 0) {
            return "No release automation state has been persisted yet.";
        }

        return `${summary.trackedRepositoryCount} repositories tracked with no active backlog.`;
    }

    return `${summary.activeRepositoryCount} repositories with backlog, ${summary.readyQueueMessages} ready queue messages, ${summary.leasedQueueMessages} leased.`;
}

function toneForReleaseSummary(summary) {
    if (
        summary.runningBuildRuns > 0 ||
        summary.runningPublishRuns > 0 ||
        summary.leasedQueueMessages > 0
    ) {
        return "ready";
    }

    if (
        summary.pendingReleaseCount > 0 ||
        summary.readyQueueMessages > 0 ||
        summary.coordinationLeaseCount > 0
    ) {
        return "warning";
    }

    return "neutral";
}

function buildReleaseHeadline(summary) {
    if (summary.pendingReleaseCount === 0) {
        return "Local release lanes are idle";
    }

    if (summary.runningBuildRuns > 0 || summary.runningPublishRuns > 0) {
        return "Release backlog is actively moving through the runtime";
    }

    if (summary.readyQueueMessages > 0) {
        return "Release backlog is waiting for the next worker claim";
    }

    return "Release backlog is blocked on local coordination";
}

function buildReleaseOverview(summary) {
    if (summary.pendingReleaseCount === 0) {
        return "No queued release currently blocks repository-local sequencing or downstream publish work.";
    }

    return `${summary.pendingReleaseCount} pending ${pluralize(summary.pendingReleaseCount, "release")} across ${summary.activeRepositoryCount} repositories, with ${summary.queuedBuildRuns} queued builds, ${summary.runningBuildRuns} running builds, ${summary.queuedPublishRuns} queued publishes, and ${summary.runningPublishRuns} running publishes.`;
}

function toneForQueueSnapshot(queue) {
    if (queue.leased_count > 0) {
        return "ready";
    }

    if (queue.ready_count > 0) {
        return "warning";
    }

    return "neutral";
}

function renderReleaseRepository(repository) {
    const repositoryTone =
        repository.running_build_runs > 0 || repository.running_publish_runs > 0
            ? "ready"
            : repository.pending_release_count > 0
                ? "warning"
                : "neutral";

    return `
        <article class="subpanel">
            <div class="subpanel-header">
                <strong>${escapeHtml(repository.repository_name)}</strong>
                <span class="pill ${repositoryTone}">
                    ${escapeHtml(`${repository.pending_release_count} pending`)}
                </span>
            </div>
            ${keyValueGrid([
        ["Enabled", repository.enabled ? "yes" : "no"],
        ["Poll interval", `${repository.polling_interval_seconds} s`],
        ["Last seen tag", repository.last_seen_tag || "not recorded"],
        ["Enabled targets", String(repository.enabled_build_target_count)],
        ["Queued builds", String(repository.queued_build_runs)],
        ["Running builds", String(repository.running_build_runs)],
        ["Queued publishes", String(repository.queued_publish_runs)],
        ["Running publishes", String(repository.running_publish_runs)],
    ])}
            <section>
                <h3>Release lane</h3>
                <div class="stack-blocks">
                    ${repository.release_queue.map(renderReleaseQueueItem).join("")}
                </div>
            </section>
        </article>
    `;
}

function renderReleaseQueueItem(release) {
    return `
        <div class="callout tone-${escapeHtml(toneForReleaseQueueItem(release))}">
            <strong>${escapeHtml(release.git_tag)}</strong>
            <p>${escapeHtml(buildReleaseQueueSummary(release))}</p>
            ${keyValueGrid([
        ["Release id", String(release.release_run_id)],
        ["Status", release.status],
        ["Unity", release.unity_version || "not resolved"],
        ["Planned", release.planned ? "yes" : "no"],
        [
            "Build lane",
            `${release.running_build_runs} running / ${release.queued_build_runs} queued / ${release.total_build_runs} total`,
        ],
        [
            "Publish lane",
            `${release.running_publish_runs} running / ${release.queued_publish_runs} queued / ${release.total_publish_runs} total`,
        ],
    ])}
        </div>
    `;
}

function toneForReleaseQueueItem(release) {
    if (release.running_build_runs > 0 || release.running_publish_runs > 0) {
        return "ready";
    }

    if (!release.planned || release.queued_build_runs > 0 || release.queued_publish_runs > 0) {
        return "warning";
    }

    return toneForStatus(release.status);
}

function buildReleaseQueueSummary(release) {
    if (!release.planned) {
        return "Release planning has not materialized build runs yet, so the repository lane is still blocked at the planning stage.";
    }

    const buildSummary = `${release.running_build_runs} running, ${release.queued_build_runs} queued, ${release.terminal_build_runs} terminal`;

    if (release.total_publish_runs === 0) {
        return `Build activity: ${buildSummary}. No publish runs have been derived yet.`;
    }

    return `Build activity: ${buildSummary}. Publish activity: ${release.running_publish_runs} running, ${release.queued_publish_runs} queued, ${release.terminal_publish_runs} terminal.`;
}

function renderDirectories(result) {
    if (!result.ok) {
        renderPanelBody("directories-panel", errorMarkup(result.error));
        return;
    }

    const directories = result.data;
    renderPanelBody(
        "directories-panel",
        Object.entries({
            Data: directories.data_dir,
            State: directories.state_dir,
            Logs: directories.logs_dir,
            Artifacts: directories.artifacts_dir,
            Runs: directories.runs_dir,
            Database: directories.database_path,
            Health: directories.health_report_path,
            Supervision: directories.supervision_contract_path,
            "Supervisor state": directories.supervisor_state_path,
            "Runtime log": directories.runtime_log_path,
        })
            .map(
                ([label, value]) => `
                    <div class="definition-row">
                        <dt>${escapeHtml(label)}</dt>
                        <dd><code>${escapeHtml(String(value))}</code></dd>
                    </div>
                `,
            )
            .join(""),
    );
}

function renderUnity(result) {
    if (!result.ok) {
        setPill("unity-pill", "Error", "error");
        renderPanelBody("unity-panel", errorMarkup(result.error));
        return;
    }

    const unity = result.data;
    const capability = unity.capability_profile;
    const readyTargets = unity.build_targets.filter(
        (target) => target.diagnostic_status === "ready",
    ).length;
    const readyEditors = capability.discovered_editors.filter(
        (editor) => editor.status === "ready",
    ).length;
    const runnerTone = toneForStatus(capability.runner_selection.status);
    setPill(
        "unity-pill",
        `${readyEditors} editors`,
        readyEditors > 0 ? runnerTone : "warning",
    );

    const rootsMarkup = unity.discovery_roots
        .map(
            (root) => `
                <li class="compact-item">
                    <span class="pill ${root.exists ? "ready" : "warning"}">
                        ${root.exists ? "present" : "missing"}
                    </span>
                    <code>${escapeHtml(root.path)}</code>
                </li>
            `,
        )
        .join("");
    const prerequisitesMarkup = capability.platform_prerequisites.length
        ? capability.platform_prerequisites
            .map(
                (tool) => `
                    <li class="compact-item">
                        <span class="pill ${escapeHtml(toneForStatus(tool.status))}">
                            ${escapeHtml(tool.status)}
                        </span>
                        <span>
                            ${escapeHtml(tool.name)}
                            ${tool.path ? `<code>${escapeHtml(tool.path)}</code>` : ""}
                        </span>
                    </li>
                `,
            )
            .join("")
        : '<li class="compact-item"><span class="pill neutral">none</span><span>No platform-specific prerequisites are currently evaluated by this runtime build.</span></li>';
    const editorsMarkup = capability.discovered_editors.length
        ? capability.discovered_editors
            .map(
                (editor) => `
                    <article class="subpanel">
                        <div class="subpanel-header">
                            <strong>${escapeHtml(editor.version)}</strong>
                            <span class="pill ${escapeHtml(toneForStatus(editor.status))}">
                                ${escapeHtml(editor.status)}
                            </span>
                        </div>
                        ${keyValueGrid([
                    ["Source", editor.source],
                    ["Install root", editor.install_root_path],
                    ["Executable", editor.executable_path],
                    [
                        "Targets",
                        editor.supported_build_targets.join(", ") || "not detected",
                    ],
                ])}
                        <p class="subpanel-copy">${escapeHtml(editor.message)}</p>
                    </article>
                `,
            )
            .join("")
        : '<div class="callout tone-warning"><strong>Discovered editors</strong><p>No Unity editors were found under the common installation roots yet.</p></div>';
    const licenseSearchMarkup = capability.unity_license.searched_paths.length
        ? `
            <ul class="compact-list">
                ${capability.unity_license.searched_paths
            .map(
                (path) => `
                        <li class="compact-item">
                            <span class="pill neutral">searched</span>
                            <code>${escapeHtml(path)}</code>
                        </li>
                    `,
            )
            .join("")}
            </ul>
        `
        : '<p class="subpanel-copy">No common Unity license paths were evaluated.</p>';

    const targetsMarkup = unity.build_targets.length
        ? unity.build_targets
            .map(
                (target) => `
                        <article class="subpanel">
                            <div class="subpanel-header">
                                <strong>
                                    ${escapeHtml(target.repository_name)} /
                                    ${escapeHtml(target.target_name)}
                                </strong>
                                <span class="pill ${escapeHtml(toneForStatus(target.diagnostic_status))}">
                                    ${escapeHtml(target.diagnostic_status)}
                                </span>
                            </div>
                            ${keyValueGrid([
                    ["Platform", target.platform],
                    ["Runner", target.runner_type],
                    ["Build method", target.build_method || "not configured"],
                    ["Enabled", target.enabled ? "yes" : "no"],
                ])}
                            <p class="subpanel-copy">${escapeHtml(target.diagnostic_message)}</p>
                        </article>
                    `,
            )
            .join("")
        : '<div class="callout tone-neutral"><strong>Build targets</strong><p>No persisted Unity build target settings were found.</p></div>';

    renderPanelBody(
        "unity-panel",
        `
            <div class="callout tone-${escapeHtml(runnerTone)}">
                <strong>Runner selection</strong>
                <p>${escapeHtml(capability.runner_selection.message)}</p>
                ${keyValueGrid([
            ["Selected runner", capability.runner_selection.selected_runner_family || "not selected"],
            ["Status", capability.runner_selection.status],
            ["Supported runner families", unity.supported_runner_families.join(", ")],
        ])}
            </div>
            <div class="subpanel-grid">
                <article class="subpanel">
                    <div class="subpanel-header">
                        <strong>Host context</strong>
                        <span class="pill neutral">${escapeHtml(unity.platform)}</span>
                    </div>
                    ${keyValueGrid([
            ["Architecture", capability.architecture],
            ["Packaging mode", capability.packaging_mode],
            ["Inside WSL", capability.inside_wsl ? "yes" : "no"],
            ["Ready editors", `${readyEditors}`],
            ["Ready targets", `${readyTargets}/${unity.build_targets.length}`],
        ])}
                </article>
                <article class="subpanel">
                    <div class="subpanel-header">
                        <strong>Git tooling</strong>
                        <span class="pill ${escapeHtml(toneForStatus(capability.git_tool.status))}">
                            ${escapeHtml(capability.git_tool.status)}
                        </span>
                    </div>
                    ${keyValueGrid([
            ["Tool", capability.git_tool.name],
            ["Path", capability.git_tool.path || "not found"],
            ["Version", capability.git_tool.version || "not detected"],
        ])}
                    <p class="subpanel-copy">${escapeHtml(capability.git_tool.message)}</p>
                </article>
                <article class="subpanel">
                    <div class="subpanel-header">
                        <strong>Unity license</strong>
                        <span class="pill ${escapeHtml(toneForStatus(capability.unity_license.status))}">
                            ${escapeHtml(capability.unity_license.status)}
                        </span>
                    </div>
                    ${keyValueGrid([
            ["Detected", capability.unity_license.exists ? "yes" : "no"],
            ["Resolved path", capability.unity_license.resolved_path || "not detected"],
        ])}
                    <p class="subpanel-copy">${escapeHtml(capability.unity_license.message)}</p>
                    ${licenseSearchMarkup}
                </article>
            </div>
            <section>
                <h3>Discovery roots</h3>
                <ul class="compact-list">${rootsMarkup}</ul>
            </section>
            <section>
                <h3>Platform prerequisites</h3>
                <ul class="compact-list">${prerequisitesMarkup}</ul>
            </section>
            <section>
                <h3>Discovered editors</h3>
                <div class="subpanel-grid">${editorsMarkup}</div>
            </section>
            <section>
                <h3>Build target diagnostics</h3>
                <div class="subpanel-grid">${targetsMarkup}</div>
            </section>
            <div class="callout tone-neutral">
                <strong>Host platform</strong>
                <p>${escapeHtml(unity.platform)}</p>
                <p class="muted">Supported runner families: ${escapeHtml(
            unity.supported_runner_families.join(", "),
        )}</p>
            </div>
        `,
    );
}

function renderSecrets(result) {
    if (!result.ok) {
        setPill("secret-pill", "Error", "error");
        renderPanelBody("secret-panel", errorMarkup(result.error));
        return;
    }

    const secrets = result.data;
    ensureSecretEditorState(secrets);
    const selectedCredential = currentSecretEditorCredential(secrets);
    setPill(
        "secret-pill",
        `${secrets.credentials.length} credentials`,
        secrets.warnings.length > 0 ? "warning" : "ready",
    );

    const warningsMarkup = secrets.warnings.length
        ? `
                <div class="callout tone-warning">
                    <strong>Warnings</strong>
                    <ul class="compact-list">
                        ${secrets.warnings
            .map((warning) => `<li>${escapeHtml(warning)}</li>`)
            .join("")}
                    </ul>
                </div>
            `
        : "";
    const actionMarkup = renderSecretActionStatus();
    const supportedKindsMarkup = secrets.supported_credential_kinds.length
        ? `
                <ul class="compact-list">
                    ${secrets.supported_credential_kinds
            .map(
                (kind) => `
                                <li class="compact-item">
                                    <span class="pill neutral">${escapeHtml(kind)}</span>
                                    <span>${escapeHtml(requiredSecretKeysForKind(kind).join(", ") || "No required keys")}</span>
                                </li>
                            `,
            )
            .join("")}
                </ul>
            `
        : '<p class="subpanel-copy">No credential kinds are currently translated by this runtime build.</p>';
    const editorTargetOptions = [
        '<option value="">Create new credential</option>',
        ...secrets.credentials.map(
            (credential) => `
                <option value="${credential.credential_id}" ${credential.credential_id === uiState.secretEditorCredentialId
                    ? "selected"
                    : ""
                }>
                    ${escapeHtml(`${credential.name} (${credential.kind})`)}
                </option>
            `,
        ),
    ].join("");
    const editorHelp = selectedCredential
        ? `Replacing ${selectedCredential.name} will overwrite its stored JSON without echoing secret values back into diagnostics.`
        : "Create a new credential entry by pasting a complete JSON object for the selected kind.";
    const credentialsMarkup = secrets.credentials.length
        ? secrets.credentials.map(renderSecretCredentialCard).join("")
        : '<div class="callout tone-neutral"><strong>Credentials</strong><p>No persisted credentials were found.</p></div>';
    const repositoryBindingsMarkup = secrets.repository_bindings.length
        ? secrets.repository_bindings
            .map((binding) =>
                renderRepositorySecretBindingCard(binding, secrets.credentials),
            )
            .join("")
        : '<div class="callout tone-neutral"><strong>Repository bindings</strong><p>No repositories were found in the runtime store yet.</p></div>';
    const publishTargetBindingsMarkup = secrets.publish_target_bindings.length
        ? secrets.publish_target_bindings
            .map((binding) =>
                renderPublishTargetSecretBindingCard(
                    binding,
                    secrets.credentials,
                ),
            )
            .join("")
        : '<div class="callout tone-neutral"><strong>Publish target bindings</strong><p>No publish targets were found in the runtime store yet.</p></div>';

    renderPanelBody(
        "secret-panel",
        `
            <div class="callout tone-neutral">
                <strong>Storage model</strong>
                <p>${escapeHtml(secrets.storage_model)}</p>
            </div>
            ${actionMarkup}
            ${warningsMarkup}
            <section>
                <h3>Credential editor</h3>
                <div class="subpanel stack-blocks">
                    <div class="callout tone-neutral">
                        <strong>Supported credential kinds</strong>
                        ${supportedKindsMarkup}
                    </div>
                    <form id="secret-editor-form" class="stack-blocks">
                        <div class="form-grid">
                            <label class="field field-span-2">
                                <span>Editor target</span>
                                <select id="secret-editor-target" ${uiState.secretActionInFlight ? "disabled" : ""}>
                                    ${editorTargetOptions}
                                </select>
                            </label>
                            <label class="field">
                                <span>Credential name</span>
                                <input
                                    id="secret-editor-name"
                                    type="text"
                                    value="${escapeHtml(uiState.secretEditorName)}"
                                    placeholder="origin-basic"
                                    ${uiState.secretActionInFlight ? "disabled" : ""}
                                />
                            </label>
                            <label class="field">
                                <span>Credential kind</span>
                                <select id="secret-editor-kind" ${uiState.secretActionInFlight ? "disabled" : ""}>
                                    ${secrets.supported_credential_kinds
            .map(
                (kind) => `
                                                <option value="${escapeHtml(kind)}" ${kind === uiState.secretEditorKind
                        ? "selected"
                        : ""
                    }>
                                                    ${escapeHtml(kind)}
                                                </option>
                                            `,
            )
            .join("")}
                                </select>
                            </label>
                            <label class="field field-span-2">
                                <span>Credential JSON</span>
                                <textarea
                                    id="secret-editor-config-json"
                                    rows="8"
                                    placeholder='{"token":"replace-me"}'
                                    ${uiState.secretActionInFlight ? "disabled" : ""}
                                >${escapeHtml(uiState.secretEditorConfigJson)}</textarea>
                            </label>
                        </div>
                        <p class="subpanel-copy">${escapeHtml(editorHelp)}</p>
                        <button class="action-button" type="submit" ${uiState.secretActionInFlight ? "disabled" : ""}>
                            ${selectedCredential ? "Update credential" : "Create credential"}
                        </button>
                    </form>
                </div>
            </section>
            <section>
                <h3>Credential entries</h3>
                <div class="subpanel-grid">${credentialsMarkup}</div>
            </section>
            <section>
                <h3>Repository bindings</h3>
                <div class="subpanel-grid">${repositoryBindingsMarkup}</div>
            </section>
            <section>
                <h3>Publish target bindings</h3>
                <div class="subpanel-grid">${publishTargetBindingsMarkup}</div>
            </section>
        `,
    );

    bindSecretManagementControls(secrets);
}

function ensureSecretEditorState(secrets) {
    const selectedCredential = currentSecretEditorCredential(secrets);
    if (uiState.secretEditorCredentialId !== null && !selectedCredential) {
        uiState.secretEditorCredentialId = null;
        uiState.secretEditorName = "";
        uiState.secretEditorConfigJson = "";
    }

    if (selectedCredential && !uiState.secretEditorName) {
        uiState.secretEditorName = selectedCredential.name;
    }

    if (
        !uiState.secretEditorKind ||
        (secrets.supported_credential_kinds.length > 0 &&
            !secrets.supported_credential_kinds.includes(uiState.secretEditorKind))
    ) {
        uiState.secretEditorKind =
            selectedCredential &&
                secrets.supported_credential_kinds.includes(selectedCredential.kind)
                ? selectedCredential.kind
                : secrets.supported_credential_kinds[0] || "";
    }
}

function currentSecretEditorCredential(secrets) {
    return (
        secrets.credentials.find(
            (credential) =>
                credential.credential_id === uiState.secretEditorCredentialId,
        ) || null
    );
}

function renderSecretActionStatus() {
    if (!uiState.secretActionStatus) {
        return "";
    }

    return `
        <div class="callout tone-${escapeHtml(uiState.secretActionStatus.tone)}">
            <strong>${escapeHtml(uiState.secretActionStatus.title)}</strong>
            <p>${escapeHtml(uiState.secretActionStatus.detail)}</p>
        </div>
    `;
}

function renderSecretCredentialCard(credential) {
    const bindingMarkup = credential.bindings.length
        ? `
                <ul class="compact-list">
                    ${credential.bindings
            .map(renderSecretBindingReference)
            .join("")}
                </ul>
            `
        : '<p class="subpanel-copy">No repository or publish target currently references this credential.</p>';

    return `
        <article class="subpanel stack-blocks">
            <div class="subpanel-header">
                <strong>${escapeHtml(credential.name)}</strong>
                <span class="pill ${escapeHtml(toneForStatus(credential.config_summary.status))}">
                    ${escapeHtml(credential.kind)}
                </span>
            </div>
            ${keyValueGrid([
        ["Storage model", credential.storage_model],
        ["Config status", credential.config_summary.status],
        [
            "Top-level keys",
            credential.config_summary.top_level_keys.join(", ") || "none",
        ],
        [
            "Missing keys",
            credential.config_summary.missing_required_keys.join(", ") ||
            "none",
        ],
        ["Bindings", String(credential.bindings.length)],
        ["Updated", optionalDateTime(credential.updated_at)],
    ])}
            <p class="subpanel-copy">${escapeHtml(credential.config_summary.message)}</p>
            ${bindingMarkup}
        </article>
    `;
}

function renderSecretBindingReference(binding) {
    return `
        <li class="compact-item">
            <span class="pill ${binding.enabled ? "ready" : "warning"}">
                ${escapeHtml(binding.binding_kind)}
            </span>
            <span>
                ${escapeHtml(binding.repository_name)} /
                ${escapeHtml(binding.binding_name)}
            </span>
        </li>
    `;
}

function renderRepositorySecretBindingCard(binding, credentials) {
    const credential = credentials.find(
        (entry) => entry.credential_id === binding.credentials_id,
    );
    const credentialLabel = credential
        ? `${credential.name} (${credential.kind})`
        : "No credential";
    const credentialTone = credential
        ? toneForStatus(credential.config_summary.status)
        : "neutral";
    const credentialMessage = credential
        ? credential.config_summary.message
        : "Public Git access or another external auth path must satisfy this repository.";

    return `
        <article class="subpanel stack-blocks">
            <div class="subpanel-header">
                <strong>${escapeHtml(binding.repository_name)}</strong>
                <span class="pill ${binding.enabled ? "ready" : "warning"}">
                    ${binding.enabled ? "enabled" : "disabled"}
                </span>
            </div>
            ${keyValueGrid([
        ["Repository id", String(binding.repository_id)],
        ["Credential", credentialLabel],
        [
            "Config status",
            credential ? credential.config_summary.status : "unbound",
        ],
    ])}
            <div class="callout tone-${escapeHtml(credentialTone)}">
                <strong>Binding health</strong>
                <p>${escapeHtml(credentialMessage)}</p>
            </div>
            <form
                class="stack-blocks secret-binding-form"
                data-secret-binding-kind="repository"
                data-secret-binding-id="${binding.repository_id}"
            >
                <label class="field">
                    <span>Credential binding</span>
                    <select name="credentials_id" ${uiState.secretActionInFlight ? "disabled" : ""}>
                        ${renderSecretBindingSelectOptions(
        credentials,
        binding.credentials_id,
    )}
                    </select>
                </label>
                <button class="action-button" type="submit" ${uiState.secretActionInFlight ? "disabled" : ""}>
                    Save repository binding
                </button>
            </form>
        </article>
    `;
}

function renderPublishTargetSecretBindingCard(binding, credentials) {
    const credential = credentials.find(
        (entry) => entry.credential_id === binding.credentials_id,
    );
    const credentialLabel = credential
        ? `${credential.name} (${credential.kind})`
        : "No credential";
    const credentialTone = credential
        ? toneForStatus(credential.config_summary.status)
        : "neutral";
    const credentialMessage = credential
        ? credential.config_summary.message
        : "No credential is bound to this publish target.";

    return `
        <article class="subpanel stack-blocks">
            <div class="subpanel-header">
                <strong>
                    ${escapeHtml(binding.repository_name)} /
                    ${escapeHtml(binding.publish_target_name)}
                </strong>
                <span class="pill ${binding.enabled ? "ready" : "warning"}">
                    ${binding.enabled ? "enabled" : "disabled"}
                </span>
            </div>
            ${keyValueGrid([
        ["Publish target id", String(binding.publish_target_id)],
        ["Kind", binding.publish_target_kind],
        ["Credential", credentialLabel],
        [
            "Config status",
            credential ? credential.config_summary.status : "unbound",
        ],
    ])}
            <div class="callout tone-${escapeHtml(credentialTone)}">
                <strong>Binding health</strong>
                <p>${escapeHtml(credentialMessage)}</p>
            </div>
            <form
                class="stack-blocks secret-binding-form"
                data-secret-binding-kind="publish_target"
                data-secret-binding-id="${binding.publish_target_id}"
            >
                <label class="field">
                    <span>Credential binding</span>
                    <select name="credentials_id" ${uiState.secretActionInFlight ? "disabled" : ""}>
                        ${renderSecretBindingSelectOptions(
        credentials,
        binding.credentials_id,
    )}
                    </select>
                </label>
                <button class="action-button" type="submit" ${uiState.secretActionInFlight ? "disabled" : ""}>
                    Save publish target binding
                </button>
            </form>
        </article>
    `;
}

function renderSecretBindingSelectOptions(credentials, selectedCredentialsId) {
    return [
        `<option value="" ${selectedCredentialsId === null ? "selected" : ""}>No credential</option>`,
        ...credentials.map(
            (credential) => `
                <option value="${credential.credential_id}" ${credential.credential_id === selectedCredentialsId
                    ? "selected"
                    : ""
                }>
                    ${escapeHtml(`${credential.name} (${credential.kind})`)}
                </option>
            `,
        ),
    ].join("");
}

function bindSecretManagementControls(secrets) {
    const editorTarget = document.getElementById("secret-editor-target");
    if (editorTarget) {
        editorTarget.addEventListener("change", (event) => {
            const value = String(event.target.value || "").trim();
            if (!value) {
                uiState.secretEditorCredentialId = null;
                uiState.secretEditorName = "";
                uiState.secretEditorKind =
                    secrets.supported_credential_kinds[0] || "";
                uiState.secretEditorConfigJson = "";
            } else {
                const credentialId = Number(value);
                const credential = secrets.credentials.find(
                    (entry) => entry.credential_id === credentialId,
                );
                if (credential) {
                    uiState.secretEditorCredentialId = credential.credential_id;
                    uiState.secretEditorName = credential.name;
                    uiState.secretEditorKind = credential.kind;
                    uiState.secretEditorConfigJson = "";
                }
            }
            uiState.secretActionStatus = null;
            rerenderSecretsOnly();
        });
    }

    const editorName = document.getElementById("secret-editor-name");
    if (editorName) {
        editorName.addEventListener("input", (event) => {
            uiState.secretEditorName = event.target.value;
        });
    }

    const editorKind = document.getElementById("secret-editor-kind");
    if (editorKind) {
        editorKind.addEventListener("change", (event) => {
            uiState.secretEditorKind = event.target.value;
        });
    }

    const editorConfigJson = document.getElementById("secret-editor-config-json");
    if (editorConfigJson) {
        editorConfigJson.addEventListener("input", (event) => {
            uiState.secretEditorConfigJson = event.target.value;
        });
    }

    const editorForm = document.getElementById("secret-editor-form");
    if (editorForm) {
        editorForm.addEventListener("submit", handleSecretCredentialSubmit);
    }

    document.querySelectorAll(".secret-binding-form").forEach((form) => {
        form.addEventListener("submit", handleSecretBindingSubmit);
    });
}

async function handleSecretCredentialSubmit(event) {
    event.preventDefault();

    const isUpdate = uiState.secretEditorCredentialId !== null;
    const succeeded = await performSecretMutation(
        COMMANDS.saveSecretCredential,
        {
            input: {
                credential_id: uiState.secretEditorCredentialId,
                name: uiState.secretEditorName,
                kind: uiState.secretEditorKind,
                config_json: uiState.secretEditorConfigJson,
            },
        },
        isUpdate ? "Updating credential" : "Creating credential",
        isUpdate ? "Credential updated" : "Credential created",
        isUpdate
            ? "The runtime store replaced the saved credential JSON without echoing secret values back into diagnostics."
            : "The runtime store recorded a new credential entry.",
    );
    if (!succeeded) {
        return;
    }

    if (isUpdate) {
        uiState.secretEditorConfigJson = "";
    } else {
        uiState.secretEditorName = "";
        uiState.secretEditorConfigJson = "";
    }

    rerenderSecretsOnly();
}

async function handleSecretBindingSubmit(event) {
    event.preventDefault();

    const form = event.currentTarget;
    const bindingKind = form.dataset.secretBindingKind;
    const bindingId = Number(form.dataset.secretBindingId);
    const credentialsValue = String(
        form.elements.credentials_id?.value || "",
    ).trim();
    const credentialsId = credentialsValue ? Number(credentialsValue) : null;

    if (bindingKind === "repository") {
        await performSecretMutation(
            COMMANDS.updateRepositorySecretBinding,
            {
                input: {
                    repository_id: bindingId,
                    credentials_id: credentialsId,
                },
            },
            "Updating repository binding",
            "Repository binding updated",
            "The repository now points at the selected credential entry.",
        );
        return;
    }

    await performSecretMutation(
        COMMANDS.updatePublishTargetSecretBinding,
        {
            input: {
                publish_target_id: bindingId,
                credentials_id: credentialsId,
            },
        },
        "Updating publish target binding",
        "Publish target binding updated",
        "The publish target now points at the selected credential entry.",
    );
}

async function performSecretMutation(
    command,
    payload,
    pendingTitle,
    successTitle,
    successDetail,
) {
    const invoker = getTauriInvoker();
    if (!invoker) {
        uiState.secretActionStatus = {
            tone: "error",
            title: "Tauri API unavailable",
            detail:
                "window.__TAURI__.core.invoke is not available. Open this page inside the desktop shell to mutate persisted secret settings.",
        };
        rerenderSecretsOnly();
        return false;
    }

    uiState.secretActionInFlight = true;
    uiState.secretActionStatus = {
        tone: "warning",
        title: pendingTitle,
        detail:
            "The desktop shell is applying a secret management change to the local runtime store.",
    };
    rerenderSecretsOnly();

    try {
        await invoker(command, payload);
        uiState.secretActionStatus = {
            tone: "ready",
            title: successTitle,
            detail: successDetail,
        };
        await refreshDiagnostics("secret-mutation");
        return true;
    } catch (error) {
        uiState.secretActionStatus = {
            tone: "error",
            title: normalizeErrorTitle(error),
            detail: normalizeErrorDetail(error),
        };
        return false;
    } finally {
        uiState.secretActionInFlight = false;
        rerenderSecretsOnly();
    }
}

function requiredSecretKeysForKind(kind) {
    if (kind === "git-http-basic") {
        return ["username", "password"];
    }

    if (kind === "git-http-bearer") {
        return ["token"];
    }

    return [];
}

function renderLogs(result) {
    if (!result.ok) {
        setPill("logs-pill", "Error", "error");
        document.getElementById("logs-meta").textContent = result.error.title;
        renderPanelBody("logs-panel", errorMarkup(result.error));
        return;
    }

    const lines = result.data;
    if (!lines.length) {
        setPill("logs-pill", "0 lines", "neutral");
        document.getElementById("logs-meta").textContent =
            `Fetched 0 lines from the last ${uiState.logLineLimit}-line window.`;
        renderPanelBody(
            "logs-panel",
            '<div class="callout tone-neutral"><strong>Runtime log</strong><p>No log lines were returned by the shell.</p></div>',
        );
        return;
    }

    const entries = lines.map(buildLogEntry);
    const filteredEntries = entries.filter(matchesLogFilters);
    const filteredLines = filteredEntries.map((entry) => entry.rawLine);
    const tone = filteredLines.length
        ? "ready"
        : hasActiveLogFilters()
            ? "warning"
            : "neutral";

    setPill("logs-pill", `${filteredLines.length}/${lines.length} lines`, tone);
    document.getElementById("logs-meta").textContent = buildLogMeta(
        lines.length,
        filteredLines.length,
    );

    if (!filteredLines.length) {
        renderPanelBody("logs-panel", buildNoLogMatchMarkup(lines.length));
        return;
    }

    renderPanelBody(
        "logs-panel",
        `<pre>${escapeHtml(filteredLines.join("\n"))}</pre>`,
    );
}

function rerenderLogsOnly() {
    if (!uiState.lastDiagnostics) {
        return;
    }

    renderLogs(uiState.lastDiagnostics.logs);
}

function rerenderSecretsOnly() {
    if (!uiState.lastDiagnostics) {
        return;
    }

    renderSecrets(uiState.lastDiagnostics.secrets);
}

function buildLogEntry(rawLine) {
    let parsed = null;

    try {
        parsed = JSON.parse(rawLine);
    } catch {
        parsed = null;
    }

    return {
        rawLine,
        level: normalizeLogLevel(parsed?.level),
        event: parsed?.event ? String(parsed.event) : "",
        message: parsed?.message ? String(parsed.message) : "",
        status: parsed?.status ? String(parsed.status) : "",
    };
}

function matchesLogFilters(entry) {
    const levelFilter = uiState.logLevelFilter;
    if (levelFilter !== DEFAULT_LOG_LEVEL_FILTER && entry.level !== levelFilter) {
        return false;
    }

    const searchQuery = uiState.logSearchQuery.trim().toLowerCase();
    if (!searchQuery) {
        return true;
    }

    const haystack = [
        entry.rawLine,
        entry.level,
        entry.event,
        entry.message,
        entry.status,
    ]
        .join(" ")
        .toLowerCase();

    return haystack.includes(searchQuery);
}

function normalizeLogLevel(level) {
    const normalized = String(level || "").trim().toLowerCase();
    if (normalized === "warning") {
        return "warn";
    }

    return normalized;
}

function buildLogMeta(totalLineCount, filteredLineCount) {
    const parts = [
        `Fetched ${totalLineCount} ${pluralize(totalLineCount, "line")} from the last ${uiState.logLineLimit}-line window.`,
    ];

    if (uiState.logLevelFilter !== DEFAULT_LOG_LEVEL_FILTER) {
        parts.push(`Level filter: ${uiState.logLevelFilter}.`);
    }

    const searchQuery = uiState.logSearchQuery.trim();
    if (searchQuery) {
        parts.push(`Search: "${searchQuery}".`);
    }

    if (filteredLineCount !== totalLineCount) {
        parts.push(
            `Showing ${filteredLineCount} matching ${pluralize(filteredLineCount, "line")}.`,
        );
    }

    return parts.join(" ");
}

function buildNoLogMatchMarkup(totalLineCount) {
    return `
        <div class="callout tone-warning">
            <strong>No matching log lines</strong>
            <p>${escapeHtml(buildNoLogMatchMessage(totalLineCount))}</p>
        </div>
    `;
}

function buildNoLogMatchMessage(totalLineCount) {
    const parts = [
        `The shell fetched ${totalLineCount} ${pluralize(totalLineCount, "line")} but none matched the current filters.`,
    ];

    if (uiState.logLevelFilter !== DEFAULT_LOG_LEVEL_FILTER) {
        parts.push(`Level filter: ${uiState.logLevelFilter}.`);
    }

    const searchQuery = uiState.logSearchQuery.trim();
    if (searchQuery) {
        parts.push(`Search: "${searchQuery}".`);
    }

    parts.push(
        "Widen the search query or change the level filter to inspect more runtime events.",
    );
    return parts.join(" ");
}

function hasActiveLogFilters() {
    return (
        uiState.logLevelFilter !== DEFAULT_LOG_LEVEL_FILTER ||
        uiState.logSearchQuery.trim().length > 0
    );
}

function renderPanelBody(id, markup) {
    document.getElementById(id).innerHTML = markup;
}

function setConnectionState(message, tone) {
    const element = document.getElementById("connection-state");
    element.textContent = message;
    element.className = `connection-state tone-${tone}`;
}

function setRefreshButtonState(disabled) {
    const button = document.getElementById("refresh-button");
    button.disabled = disabled;
    button.textContent = disabled ? "Refreshing diagnostics" : "Refresh diagnostics";
}

function updatePollingStateMessage() {
    const element = document.getElementById("poll-state");

    if (uiState.refreshInFlight) {
        element.textContent = "A runtime probe is in flight.";
        return;
    }

    if (!uiState.autoRefreshEnabled) {
        element.textContent =
            "Automatic refresh is disabled. Use the button to fetch a fresh runtime snapshot.";
        return;
    }

    if (document.hidden) {
        element.textContent =
            "Automatic refresh is paused while this window is hidden.";
        return;
    }

    if (uiState.lastDiagnostics?.connected === false) {
        element.textContent =
            "Automatic refresh is paused until the Tauri API is available.";
        return;
    }

    element.textContent = `Automatic refresh every ${formatDuration(
        uiState.refreshIntervalMillis,
    )}.`;
}

function setPill(id, label, tone) {
    const element = document.getElementById(id);
    element.textContent = label;
    element.className = `pill ${tone}`;
}

function connectionPendingMessage(reason) {
    if (reason === "poll" || reason === "resume") {
        return "Refreshing runtime diagnostics";
    }

    return "Loading runtime diagnostics";
}

function loadingMarkup(message) {
    return `<div class="callout tone-neutral"><strong>Loading</strong><p>${escapeHtml(message)}</p></div>`;
}

function errorMarkup(error) {
    return `
        <div class="callout tone-error">
            <strong>${escapeHtml(error.title)}</strong>
            <p>${escapeHtml(error.detail)}</p>
        </div>
    `;
}

function keyValueGrid(entries) {
    return `
        <dl class="key-value-grid">
            ${entries
            .map(
                ([label, value]) => `
                        <div>
                            <dt>${escapeHtml(label)}</dt>
                            <dd>${escapeHtml(String(value))}</dd>
                        </div>
                    `,
            )
            .join("")}
        </dl>
    `;
}

function booleanChecklist(entries) {
    return `
        <ul class="checklist">
            ${entries
            .map(
                ([label, value]) => `
                        <li>
                            <span class="pill ${value ? "ready" : "warning"}">${value ? "yes" : "no"}</span>
                            <span>${escapeHtml(label)}</span>
                        </li>
                    `,
            )
            .join("")}
        </ul>
    `;
}

function commandLabel(command) {
    const parts = [command.program, ...(command.args || [])].filter(Boolean);
    return parts.join(" ");
}

function formatDateTime(value) {
    const date = value instanceof Date ? value : new Date(value);
    return new Intl.DateTimeFormat("en-US", {
        dateStyle: "medium",
        timeStyle: "medium",
    }).format(date);
}

function formatUnixSeconds(value) {
    if (typeof value !== "number") {
        return "not reported";
    }

    return formatDateTime(value * 1000);
}

function optionalDateTime(value) {
    if (!value) {
        return "not reported";
    }

    const parsed = new Date(value);
    if (Number.isNaN(parsed.getTime())) {
        return String(value);
    }

    return formatDateTime(parsed);
}

function formatUnixMilliseconds(value) {
    if (typeof value !== "number") {
        return "not reported";
    }

    return formatDateTime(value);
}

function optionalNumber(value) {
    return value === null || value === undefined ? "not reported" : String(value);
}

function formatByteSize(value) {
    if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
        return "not reported";
    }

    if (value < 1024) {
        return `${value} B`;
    }

    const units = ["KB", "MB", "GB", "TB"];
    let size = value;
    let unitIndex = -1;
    while (size >= 1024 && unitIndex < units.length - 1) {
        size /= 1024;
        unitIndex += 1;
    }

    const digits = size >= 10 || unitIndex === 0 ? 1 : 2;
    return `${size.toFixed(digits)} ${units[unitIndex]}`;
}

function formatDuration(milliseconds) {
    const seconds = Math.round(milliseconds / 1000);
    if (seconds < 60) {
        return `${seconds} ${pluralize(seconds, "second")}`;
    }

    const minutes = Math.round(seconds / 60);
    return `${minutes} ${pluralize(minutes, "minute")}`;
}

function pluralize(count, singular) {
    return count === 1 ? singular : `${singular}s`;
}

function toneForStatus(status) {
    const normalized = String(status || "").toLowerCase();
    if (
        normalized.includes("healthy") ||
        normalized.includes("ready") ||
        normalized.includes("running") ||
        normalized.includes("succeed")
    ) {
        return "ready";
    }

    if (
        normalized.includes("warning") ||
        normalized.includes("restart") ||
        normalized.includes("recover") ||
        normalized.includes("shutting") ||
        normalized.includes("incomplete") ||
        normalized.includes("missing") ||
        normalized.includes("queued") ||
        normalized.includes("pending") ||
        normalized.includes("detected")
    ) {
        return "warning";
    }

    if (
        normalized.includes("error") ||
        normalized.includes("failed") ||
        normalized.includes("invalid") ||
        normalized.includes("unhealthy") ||
        normalized.includes("unsupported")
    ) {
        return "error";
    }

    return "neutral";
}

function escapeHtml(value) {
    return String(value)
        .replaceAll("&", "&amp;")
        .replaceAll("<", "&lt;")
        .replaceAll(">", "&gt;")
        .replaceAll('"', "&quot;")
        .replaceAll("'", "&#39;");
}