import { invoke } from "@tauri-apps/api/core";

export type AuthProviderStatus = {
    provider_id: string;
    label: string;
    status: string;
    status_message: string;
    instance_url: string;
    credential_id: number | null;
    credential_name: string | null;
    bound_repository_count: number;
};

export async function loadAuthProviders(): Promise<AuthProviderStatus[]> {
    return invoke<AuthProviderStatus[]>("auth_providers");
}

export async function loginWithGithubAuth(): Promise<AuthProviderStatus> {
    return invoke<AuthProviderStatus>("login_github_auth");
}
