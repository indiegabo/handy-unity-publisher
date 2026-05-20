import {
    MOCK_AUTH_PROVIDERS,
    MOCK_PROCESS_FEED_PAGE,
    MOCK_REPOSITORY_INSPECTION,
    MOCK_RUNTIME_HEALTH,
} from "./mockData";

let pinned = false;
let authProviders = cloneValue(MOCK_AUTH_PROVIDERS);
let runtimeHealth = cloneValue(MOCK_RUNTIME_HEALTH);

export async function invoke<T = unknown>(
    command: string,
    payload?: Record<string, unknown>,
): Promise<T> {
    switch (command) {
        case "auth_providers":
            return cloneValue(authProviders) as T;
        case "close_main_window":
            return undefined as T;
        case "login_github_auth":
            authProviders = [
                {
                    ...authProviders[0],
                    bound_repository_count: 2,
                    status: "connected",
                    status_message: "Browser login was refreshed successfully.",
                },
            ];
            return cloneValue(authProviders[0]) as T;
        case "main_window_pin_state":
            return pinned as T;
        case "process_feed":
            return cloneValue(MOCK_PROCESS_FEED_PAGE) as T;
        case "repository_inspection":
            return cloneValue(MOCK_REPOSITORY_INSPECTION) as T;
        case "request_repository_instant_check":
            return undefined as T;
        case "restart_runtime":
            runtimeHealth = {
                ...runtimeHealth,
                status: "healthy",
            };
            return undefined as T;
        case "runtime_health":
            return cloneValue(runtimeHealth) as T;
        case "set_main_window_pinned":
            pinned = Boolean(payload?.pinned);
            return pinned as T;
        case "start_runtime":
            runtimeHealth = {
                ...runtimeHealth,
                status: "healthy",
            };
            return undefined as T;
        case "stop_runtime":
            runtimeHealth = {
                ...runtimeHealth,
                status: "stopped",
            };
            return undefined as T;
        case "transition_window_focus":
            return undefined as T;
        default:
            throw new Error(
                `E2E mock does not implement the Tauri command \"${command}\".`,
            );
    }
}

function cloneValue<T>(value: T): T {
    return JSON.parse(JSON.stringify(value)) as T;
}