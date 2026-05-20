import { invoke } from "@tauri-apps/api/core";

export type AuthProviderStatus = {
    provider_id: string;
    label: string;
    status: string;
    status_message: string;
    instance_url: string;
    credential_id: number | null;
    credential_name: string | null;
    credential_created_at: string | null;
    credential_updated_at: string | null;
    bound_repository_count: number;
};

export async function loadAuthProviders(): Promise<AuthProviderStatus[]> {
    return invoke<AuthProviderStatus[]>("auth_providers");
}

export type LoginWithGithubAuthOptions = {
    force?: boolean;
};

export async function loginWithGithubAuth(
    options: LoginWithGithubAuthOptions = {},
): Promise<AuthProviderStatus> {
    return invoke<AuthProviderStatus>("login_github_auth", {
        force: options.force ?? false,
    });
}
