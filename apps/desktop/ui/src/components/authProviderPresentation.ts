import type { BadgeTone } from "./Surface";
import type { AuthProviderStatus } from "../services/auth";

export type AuthProviderConnectionResult = {
    message: string;
    outcome: "connected" | "reconnected";
    provider: AuthProviderStatus;
    sessionEventLabel: string;
};

export type AuthProviderLifecycleSnapshot = {
    lifecycleLabel: string;
    nextActionLabel: string;
    refreshedAtLabel: string;
    storedAtLabel: string;
};

type AuthProviderSummaryItem = {
    label: string;
    value: string;
};

type AuthProviderSummaryRow = readonly AuthProviderSummaryItem[];

export function buildAuthProviderConnectionResult(
    previousProvider: AuthProviderStatus,
    nextProvider: AuthProviderStatus,
): AuthProviderConnectionResult {
    const outcome =
        previousProvider.status === "connected" ? "reconnected" : "connected";
    const boundRepositorySummary = buildBoundRepositorySummary(
        nextProvider.bound_repository_count,
    );

    return {
        message:
            outcome === "reconnected"
                ? `${nextProvider.label} browser reconnect completed. ${boundRepositorySummary}`
                : `${nextProvider.label} browser login completed. ${boundRepositorySummary}`,
        outcome,
        provider: nextProvider,
        sessionEventLabel:
            outcome === "reconnected"
                ? "Browser reconnect completed in this session"
                : "Browser login completed in this session",
    };
}

export function buildAuthProviderLifecycleSnapshot(
    provider: AuthProviderStatus,
    connectionResult?: AuthProviderConnectionResult,
): AuthProviderLifecycleSnapshot {
    return {
        lifecycleLabel: connectionResult
            ? connectionResult.sessionEventLabel
            : buildDefaultLifecycleLabel(provider.status),
        nextActionLabel: connectionResult
            ? provider.status === "connected"
                ? "Reuse the host credential until repository access fails again"
                : "Run the browser flow again to repair the host binding"
            : buildDefaultNextActionLabel(provider.status),
        refreshedAtLabel: formatAuthProviderRefreshTimestamp(provider),
        storedAtLabel: formatAuthProviderStoredTimestamp(provider),
    };
}

export function buildAuthProviderSummaryRows(
    provider: AuthProviderStatus,
    lifecycleSnapshot: AuthProviderLifecycleSnapshot,
    options: {
        includeLifecycleRow?: boolean;
    } = {},
): readonly AuthProviderSummaryRow[] {
    const { includeLifecycleRow = true } = options;
    const rows: AuthProviderSummaryRow[] = [
        [
            {
                label: "Credential",
                value: provider.credential_name || "No reusable credential",
            },
            {
                label: "Usage",
                value: formatBoundRepositoryCount(provider.bound_repository_count),
            },
        ],
        [
            {
                label: "Stored",
                value: lifecycleSnapshot.storedAtLabel,
            },
            {
                label: "Refreshed",
                value: lifecycleSnapshot.refreshedAtLabel,
            },
        ],
    ];

    if (includeLifecycleRow) {
        rows.push([
            {
                label: "Lifecycle",
                value: lifecycleSnapshot.lifecycleLabel,
            },
            {
                label: "Next step",
                value: lifecycleSnapshot.nextActionLabel,
            },
        ]);
    }

    return rows;
}

export function buildAuthProviderActionLabel(
    provider: AuthProviderStatus,
    connectionResult?: AuthProviderConnectionResult,
) {
    if (connectionResult) {
        return provider.status === "connected"
            ? "Review recent reconnect"
            : "Review recovery flow";
    }

    return provider.status === "connected"
        ? "Review reconnect"
        : "Review connection";
}

export function resolveAuthProviderTone(status: string): BadgeTone {
    switch (status) {
        case "connected":
            return "strong";
        case "disconnected":
            return "neutral";
        default:
            return "muted";
    }
}

export function formatAuthProviderStatus(status: string) {
    switch (status) {
        case "connected":
            return "connected";
        case "disconnected":
            return "ready to connect";
        default:
            return "unavailable";
    }
}

export function formatBoundRepositoryCount(boundRepositoryCount: number) {
    return `${boundRepositoryCount} repository project${boundRepositoryCount === 1 ? "" : "s"
        }`;
}

function buildBoundRepositorySummary(boundRepositoryCount: number) {
    const boundRepositoryLabel = formatBoundRepositoryCount(boundRepositoryCount);

    return `${boundRepositoryLabel} ${boundRepositoryCount === 1 ? "is" : "are"
        } currently bound to it.`;
}

function buildDefaultLifecycleLabel(status: string) {
    switch (status) {
        case "connected":
            return "Host credential already bound on this machine";
        case "disconnected":
            return "No host-backed credential is currently bound";
        default:
            return "Host tooling is not ready for this provider";
    }
}

function buildDefaultNextActionLabel(status: string) {
    switch (status) {
        case "connected":
            return "Reopen the connection flow only after a real auth failure";
        case "disconnected":
            return "Run the browser flow to create the reusable binding";
        default:
            return "Verify Git Credential Manager availability first";
    }
}

function formatAuthProviderStoredTimestamp(provider: AuthProviderStatus) {
    if (!provider.credential_created_at) {
        return "No reusable credential";
    }

    return formatAuthProviderTimestamp(provider.credential_created_at);
}

function formatAuthProviderRefreshTimestamp(provider: AuthProviderStatus) {
    if (!provider.credential_created_at) {
        return "No reusable credential";
    }

    if (!provider.credential_updated_at) {
        return "No refresh recorded";
    }

    if (provider.credential_updated_at === provider.credential_created_at) {
        return "No separate refresh recorded";
    }

    return formatAuthProviderTimestamp(provider.credential_updated_at);
}

function formatAuthProviderTimestamp(value: string) {
    const parsed = new Date(value);

    if (Number.isNaN(parsed.getTime())) {
        return value;
    }

    const year = parsed.getUTCFullYear();
    const month = String(parsed.getUTCMonth() + 1).padStart(2, "0");
    const day = String(parsed.getUTCDate()).padStart(2, "0");
    const hours = String(parsed.getUTCHours()).padStart(2, "0");
    const minutes = String(parsed.getUTCMinutes()).padStart(2, "0");

    return `${year}-${month}-${day} ${hours}:${minutes} UTC`;
}