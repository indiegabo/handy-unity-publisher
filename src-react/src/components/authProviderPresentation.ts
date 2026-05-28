import type { BadgeTone } from "./Surface";
import type { AuthProviderStatus } from "../services/auth";

export type AuthProviderTranslate = (
    key: string,
    fallback: string,
    values?: Record<string, string | number>,
) => string;

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
    translate: AuthProviderTranslate,
    previousProvider: AuthProviderStatus,
    nextProvider: AuthProviderStatus,
): AuthProviderConnectionResult {
    const outcome =
        previousProvider.status === "connected" ? "reconnected" : "connected";
    const boundRepositorySummary = buildBoundRepositorySummary(
        translate,
        nextProvider.bound_repository_count,
    );

    return {
        message:
            outcome === "reconnected"
                ? translate(
                    "auth_providers.presentation.connection_result.reconnected",
                    "{{providerLabel}} browser reconnect completed. {{boundRepositorySummary}}",
                    {
                        boundRepositorySummary,
                        providerLabel: nextProvider.label,
                    },
                )
                : translate(
                    "auth_providers.presentation.connection_result.connected",
                    "{{providerLabel}} browser login completed. {{boundRepositorySummary}}",
                    {
                        boundRepositorySummary,
                        providerLabel: nextProvider.label,
                    },
                ),
        outcome,
        provider: nextProvider,
        sessionEventLabel:
            outcome === "reconnected"
                ? translate(
                    "auth_providers.presentation.session_event.reconnected",
                    "Browser reconnect completed in this session",
                )
                : translate(
                    "auth_providers.presentation.session_event.connected",
                    "Browser login completed in this session",
                ),
    };
}

export function buildAuthProviderLifecycleSnapshot(
    translate: AuthProviderTranslate,
    provider: AuthProviderStatus,
    connectionResult?: AuthProviderConnectionResult,
): AuthProviderLifecycleSnapshot {
    return {
        lifecycleLabel: connectionResult
            ? connectionResult.sessionEventLabel
            : buildDefaultLifecycleLabel(translate, provider.status),
        nextActionLabel: connectionResult
            ? provider.status === "connected"
                ? translate(
                    "auth_providers.presentation.next_action.connected_session",
                    "Reuse the host credential until repository access fails again",
                )
                : translate(
                    "auth_providers.presentation.next_action.disconnected_session",
                    "Run the browser flow again to repair the host binding",
                )
            : buildDefaultNextActionLabel(translate, provider.status),
        refreshedAtLabel: formatAuthProviderRefreshTimestamp(translate, provider),
        storedAtLabel: formatAuthProviderStoredTimestamp(translate, provider),
    };
}

export function buildAuthProviderSummaryRows(
    translate: AuthProviderTranslate,
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
                label: translate(
                    "auth_providers.presentation.summary.credential",
                    "Credential",
                ),
                value:
                    provider.credential_name ||
                    resolveNoReusableCredentialLabel(translate),
            },
            {
                label: translate(
                    "auth_providers.presentation.summary.usage",
                    "Usage",
                ),
                value: formatBoundRepositoryCount(
                    translate,
                    provider.bound_repository_count,
                ),
            },
        ],
        [
            {
                label: translate(
                    "auth_providers.presentation.summary.stored",
                    "Stored",
                ),
                value: lifecycleSnapshot.storedAtLabel,
            },
            {
                label: translate(
                    "auth_providers.presentation.summary.refreshed",
                    "Refreshed",
                ),
                value: lifecycleSnapshot.refreshedAtLabel,
            },
        ],
    ];

    if (includeLifecycleRow) {
        rows.push([
            {
                label: translate(
                    "auth_providers.presentation.summary.lifecycle",
                    "Lifecycle",
                ),
                value: lifecycleSnapshot.lifecycleLabel,
            },
            {
                label: translate(
                    "auth_providers.presentation.summary.next_step",
                    "Next step",
                ),
                value: lifecycleSnapshot.nextActionLabel,
            },
        ]);
    }

    return rows;
}

export function buildAuthProviderActionLabel(
    translate: AuthProviderTranslate,
    provider: AuthProviderStatus,
    connectionResult?: AuthProviderConnectionResult,
) {
    if (connectionResult) {
        return provider.status === "connected"
            ? translate(
                "auth_providers.presentation.action.review_recent_reconnect",
                "Review recent reconnect",
            )
            : translate(
                "auth_providers.presentation.action.review_recovery_flow",
                "Review recovery flow",
            );
    }

    return provider.status === "connected"
        ? translate(
            "auth_providers.presentation.action.review_reconnect",
            "Review reconnect",
        )
        : translate(
            "auth_providers.presentation.action.review_connection",
            "Review connection",
        );
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

export function formatAuthProviderStatus(
    translate: AuthProviderTranslate,
    status: string,
) {
    switch (status) {
        case "connected":
            return translate(
                "auth_providers.presentation.status.connected",
                "connected",
            );
        case "disconnected":
            return translate(
                "auth_providers.presentation.status.disconnected",
                "ready to connect",
            );
        default:
            return translate(
                "auth_providers.presentation.status.unavailable",
                "unavailable",
            );
    }
}

export function formatBoundRepositoryCount(
    translate: AuthProviderTranslate,
    boundRepositoryCount: number,
) {
    if (boundRepositoryCount === 1) {
        return translate(
            "auth_providers.presentation.bound_repository.one",
            "1 repository project",
        );
    }

    return translate(
        "auth_providers.presentation.bound_repository.other",
        "{{count}} repository projects",
        {
            count: boundRepositoryCount,
        },
    );
}

function buildBoundRepositorySummary(
    translate: AuthProviderTranslate,
    boundRepositoryCount: number,
) {
    if (boundRepositoryCount === 1) {
        return translate(
            "auth_providers.presentation.bound_summary.one",
            "1 repository project is currently bound to it.",
        );
    }

    return translate(
        "auth_providers.presentation.bound_summary.other",
        "{{count}} repository projects are currently bound to it.",
        {
            count: boundRepositoryCount,
        },
    );
}

function buildDefaultLifecycleLabel(
    translate: AuthProviderTranslate,
    status: string,
) {
    switch (status) {
        case "connected":
            return translate(
                "auth_providers.presentation.lifecycle.connected",
                "Host credential already bound on this machine",
            );
        case "disconnected":
            return translate(
                "auth_providers.presentation.lifecycle.disconnected",
                "No host-backed credential is currently bound",
            );
        default:
            return translate(
                "auth_providers.presentation.lifecycle.unavailable",
                "Host tooling is not ready for this provider",
            );
    }
}

function buildDefaultNextActionLabel(
    translate: AuthProviderTranslate,
    status: string,
) {
    switch (status) {
        case "connected":
            return translate(
                "auth_providers.presentation.next_action.connected",
                "Reopen the connection flow only after a real auth failure",
            );
        case "disconnected":
            return translate(
                "auth_providers.presentation.next_action.disconnected",
                "Run the browser flow to create the reusable binding",
            );
        default:
            return translate(
                "auth_providers.presentation.next_action.unavailable",
                "Verify Git Credential Manager availability first",
            );
    }
}

function formatAuthProviderStoredTimestamp(
    translate: AuthProviderTranslate,
    provider: AuthProviderStatus,
) {
    if (!provider.credential_created_at) {
        return resolveNoReusableCredentialLabel(translate);
    }

    return formatAuthProviderTimestamp(provider.credential_created_at);
}

function formatAuthProviderRefreshTimestamp(
    translate: AuthProviderTranslate,
    provider: AuthProviderStatus,
) {
    if (!provider.credential_created_at) {
        return resolveNoReusableCredentialLabel(translate);
    }

    if (!provider.credential_updated_at) {
        return translate(
            "auth_providers.presentation.no_refresh_recorded",
            "No refresh recorded",
        );
    }

    if (provider.credential_updated_at === provider.credential_created_at) {
        return translate(
            "auth_providers.presentation.no_separate_refresh_recorded",
            "No separate refresh recorded",
        );
    }

    return formatAuthProviderTimestamp(provider.credential_updated_at);
}

function resolveNoReusableCredentialLabel(translate: AuthProviderTranslate) {
    return translate(
        "auth_providers.presentation.no_reusable_credential",
        "No reusable credential",
    );
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