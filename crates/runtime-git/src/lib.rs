//! Wraps the local Git CLI for authenticated tag discovery and deterministic
//! workspace synchronization used by runtime automation flows.

#![forbid(unsafe_code)]

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};

/// Identifies Git credentials backed by HTTP basic authentication.
pub const KIND_GIT_HTTP_BASIC: &str = "git-http-basic";

/// Identifies Git credentials backed by HTTP bearer authentication.
pub const KIND_GIT_HTTP_BEARER: &str = "git-http-bearer";

/// Identifies GitHub credentials delegated to the host Git credential helper.
pub const KIND_GIT_HTTP_GITHUB_HOST_LOGIN: &str =
    "git-http-github-host-login";

const GIT_TERMINAL_PROMPT_ENV: &str = "GIT_TERMINAL_PROMPT";
const GIT_TERMINAL_PROMPT_DISABLED: &str = "0";
const GCM_INTERACTIVE_ENV: &str = "GCM_INTERACTIVE";
const GCM_INTERACTIVE_NEVER: &str = "never";
const GIT_ASKPASS_ENV: &str = "GIT_ASKPASS";
const SSH_ASKPASS_ENV: &str = "SSH_ASKPASS";
const GIT_CREDENTIAL_HELPER_RESET: &str = "credential.helper=";
const GIT_CORE_ASKPASS_RESET: &str = "core.askPass=";
const GIT_CREDENTIAL_INTERACTIVE_DISABLED: &str = "credential.interactive=false";
const GIT_CREDENTIAL_MANAGER_HELPER: &str = "manager";
const DEFAULT_GITHUB_INSTANCE_URL: &str = "https://github.com";
const REPOSITORY_PROVIDER_GITHUB: &str = "github";
const REPOSITORY_PROVIDER_GITLAB: &str = "gitlab";
const REPOSITORY_PROVIDER_BITBUCKET: &str = "bitbucket";
const REPOSITORY_PROVIDER_UNKNOWN: &str = "unknown";
const REPOSITORY_VISIBILITY_PUBLIC: &str = "public";
const REPOSITORY_VISIBILITY_PRIVATE: &str = "private";
const REPOSITORY_VISIBILITY_INVALID: &str = "invalid";
const REPOSITORY_VISIBILITY_UNKNOWN: &str = "unknown";
const REPOSITORY_AUTH_REQUIREMENT_NONE: &str = "none";
const REPOSITORY_AUTH_REQUIREMENT_REQUIRED: &str = "required";
const REPOSITORY_AUTH_REQUIREMENT_UNKNOWN: &str = "unknown";
const REPOSITORY_AUTH_STATUS_NOT_REQUIRED: &str = "not_required";
const REPOSITORY_AUTH_STATUS_REQUIRED_UNBOUND: &str = "required_unbound";
const REPOSITORY_AUTH_STATUS_UNSUPPORTED: &str = "unsupported";
const REPOSITORY_AUTH_STATUS_UNKNOWN: &str = "unknown";
#[cfg(test)]
const TEST_GIT_EXECUTABLE_ENV: &str =
    "HANDY_GAMES_PUBLISHER_TEST_GIT_EXECUTABLE";

/// Lists the Git transport strategy supported by the runtime scaffold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitTooling {
    LocalCli,
}

impl GitTooling {
    /// Returns the operator-facing label for the selected Git strategy.
    pub const fn label(self) -> &'static str {
        match self {
            Self::LocalCli => "local-cli",
        }
    }
}

/// Describes the Git CLI authentication flags required for one repository operation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitAuthOptions {
    pub extra_headers: Vec<String>,
    pub credential_helper: Option<String>,
    pub preserve_credential_helper: bool,
}

impl GitAuthOptions {
    /// Prefixes one Git command with the configured authentication headers.
    pub fn append_git_args<I, S>(&self, args: I) -> Vec<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut resolved = Vec::new();
        for header in &self.extra_headers {
            let trimmed = header.trim();
            if trimmed.is_empty() {
                continue;
            }

            resolved.push(String::from("-c"));
            resolved.push(format!("http.extraHeader={trimmed}"));
        }

        for argument in args {
            resolved.push(argument.as_ref().to_owned());
        }

        resolved
    }
}

/// Reports the access classification produced from one repository URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryAccessAssessment {
    pub provider_id: String,
    pub provider_label: String,
    pub instance_url: String,
    pub normalized_url: String,
    pub visibility: String,
    pub auth_requirement: String,
    pub auth_status: String,
    pub supports_interactive_login: bool,
    pub message: String,
}

/// Reports the provider identity inferred from one repository URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryProviderDetection {
    pub provider_id: String,
    pub provider_label: String,
    pub instance_url: String,
    pub normalized_url: String,
    pub supports_interactive_login: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DetectedRepositoryProvider {
    provider_id: &'static str,
    provider_label: &'static str,
    supports_interactive_login: bool,
    instance_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedRepositoryUrl {
    normalized_url: String,
    host: String,
    instance_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnonymousProbeClassification {
    AuthRequired,
    Invalid,
    Unknown,
}

/// Assesses repository provider identity and anonymous access requirements.
pub fn assess_repository_access(repository_url: &str) -> io::Result<RepositoryAccessAssessment> {
    let normalized = normalize_repository_url(repository_url)?;
    let provider = detect_repository_provider(&normalized);

    match probe_repository_anonymously(&normalized.normalized_url) {
        Ok(()) => Ok(RepositoryAccessAssessment {
            provider_id: String::from(provider.provider_id),
            provider_label: String::from(provider.provider_label),
            instance_url: provider.instance_url,
            normalized_url: normalized.normalized_url,
            visibility: String::from(REPOSITORY_VISIBILITY_PUBLIC),
            auth_requirement: String::from(REPOSITORY_AUTH_REQUIREMENT_NONE),
            auth_status: String::from(REPOSITORY_AUTH_STATUS_NOT_REQUIRED),
            supports_interactive_login: provider.supports_interactive_login,
            message: String::from(
                "Public repository detected through anonymous remote access.",
            ),
        }),
        Err(error) => match classify_anonymous_probe_failure(&error) {
            AnonymousProbeClassification::AuthRequired => {
                let auth_status = if provider.supports_interactive_login {
                    REPOSITORY_AUTH_STATUS_REQUIRED_UNBOUND
                } else {
                    REPOSITORY_AUTH_STATUS_UNSUPPORTED
                };
                let message = if provider.supports_interactive_login {
                    format!(
                        "{} repository requires authentication before HGP can access it.",
                        provider.provider_label
                    )
                } else {
                    format!(
                        "Repository requires authentication, but the current shell does not provide interactive login for {} yet.",
                        provider.provider_label
                    )
                };

                Ok(RepositoryAccessAssessment {
                    provider_id: String::from(provider.provider_id),
                    provider_label: String::from(provider.provider_label),
                    instance_url: provider.instance_url,
                    normalized_url: normalized.normalized_url,
                    visibility: String::from(REPOSITORY_VISIBILITY_PRIVATE),
                    auth_requirement: String::from(REPOSITORY_AUTH_REQUIREMENT_REQUIRED),
                    auth_status: String::from(auth_status),
                    supports_interactive_login: provider.supports_interactive_login,
                    message,
                })
            }
            AnonymousProbeClassification::Invalid => Ok(RepositoryAccessAssessment {
                provider_id: String::from(provider.provider_id),
                provider_label: String::from(provider.provider_label),
                instance_url: provider.instance_url,
                normalized_url: normalized.normalized_url,
                visibility: String::from(REPOSITORY_VISIBILITY_INVALID),
                auth_requirement: String::from(REPOSITORY_AUTH_REQUIREMENT_UNKNOWN),
                auth_status: String::from(REPOSITORY_AUTH_STATUS_UNKNOWN),
                supports_interactive_login: provider.supports_interactive_login,
                message: format!(
                    "Repository URL could not be resolved as a reachable Git remote: {error}"
                ),
            }),
            AnonymousProbeClassification::Unknown => Ok(RepositoryAccessAssessment {
                provider_id: String::from(provider.provider_id),
                provider_label: String::from(provider.provider_label),
                instance_url: provider.instance_url,
                normalized_url: normalized.normalized_url,
                visibility: String::from(REPOSITORY_VISIBILITY_UNKNOWN),
                auth_requirement: String::from(REPOSITORY_AUTH_REQUIREMENT_UNKNOWN),
                auth_status: String::from(REPOSITORY_AUTH_STATUS_UNKNOWN),
                supports_interactive_login: provider.supports_interactive_login,
                message: format!(
                    "HGP could not determine whether the repository is public or private from anonymous access: {error}"
                ),
            }),
        },
    }
}

/// Detects the repository provider from URL heuristics without probing visibility.
pub fn detect_repository_provider_from_url(
    repository_url: &str,
) -> io::Result<RepositoryProviderDetection> {
    let normalized = normalize_repository_url(repository_url)?;
    let provider = detect_repository_provider(&normalized);

    Ok(RepositoryProviderDetection {
        provider_id: String::from(provider.provider_id),
        provider_label: String::from(provider.provider_label),
        instance_url: provider.instance_url,
        normalized_url: normalized.normalized_url,
        supports_interactive_login: provider.supports_interactive_login,
    })
}

/// Receives coarse checkout progress updates emitted by the Git workspace syncer.
pub trait GitProgressReporter {
    fn report(&mut self, message: &str);
}

#[derive(Debug, Default)]
struct NoopGitProgressReporter;

impl GitProgressReporter for NoopGitProgressReporter {
    fn report(&mut self, _message: &str) {}
}

/// Resolves stored credentials into Git CLI authentication headers.
pub fn git_auth_options_from_credentials(
    kind: &str,
    config_json: &str,
) -> io::Result<GitAuthOptions> {
    git_auth_options_from_credentials_with_github_header_resolver(
        kind,
        config_json,
        resolve_github_host_login_auth_header,
    )
}

fn git_auth_options_from_credentials_with_github_header_resolver<F>(
    kind: &str,
    config_json: &str,
    resolve_github_header: F,
) -> io::Result<GitAuthOptions>
where
    F: Fn(&str, Option<&str>) -> io::Result<String>,
{
    let kind = require_non_empty(kind, "credentials kind")?;

    match kind.as_str() {
        KIND_GIT_HTTP_BASIC => {
            #[derive(Deserialize)]
            struct BasicConfig {
                username: String,
                password: String,
            }

            let config: BasicConfig = decode_auth_config(config_json)?;
            let username = require_non_empty(&config.username, "git basic username")?;
            let password = require_non_empty(&config.password, "git basic password")?;
            let token = BASE64_STANDARD.encode(format!("{username}:{password}"));

            Ok(GitAuthOptions {
                extra_headers: vec![format!("Authorization: Basic {token}")],
                credential_helper: None,
                preserve_credential_helper: false,
            })
        }
        KIND_GIT_HTTP_BEARER => {
            #[derive(Deserialize)]
            struct BearerConfig {
                token: String,
            }

            let config: BearerConfig = decode_auth_config(config_json)?;
            let token = require_non_empty(&config.token, "git bearer token")?;

            Ok(GitAuthOptions {
                extra_headers: vec![format!("Authorization: Bearer {token}")],
                credential_helper: None,
                preserve_credential_helper: false,
            })
        }
        KIND_GIT_HTTP_GITHUB_HOST_LOGIN => {
            #[derive(Deserialize)]
            struct GithubHostLoginConfig {
                provider: String,
                #[serde(default)]
                instance_url: Option<String>,
                #[serde(default)]
                login: Option<String>,
            }

            let config: GithubHostLoginConfig = decode_auth_config(config_json)?;
            let provider =
                require_non_empty(&config.provider, "GitHub host login provider")?;
            if !provider.eq_ignore_ascii_case("github") {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "GitHub host login credentials must declare provider \"github\", found {provider:?}"
                    ),
                ));
            }

            let instance_url = normalized_optional_string(config.instance_url.as_deref())
                .unwrap_or_else(|| String::from(DEFAULT_GITHUB_INSTANCE_URL));
            let login = normalized_optional_string(config.login.as_deref());
            let auth_header = resolve_github_header(&instance_url, login.as_deref())?;

            Ok(GitAuthOptions {
                extra_headers: vec![auth_header],
                credential_helper: None,
                preserve_credential_helper: false,
            })
        }
        _ => Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("unsupported credentials kind {kind:?}"),
        )),
    }
}

fn resolve_github_host_login_auth_header(
    instance_url: &str,
    login: Option<&str>,
) -> io::Result<String> {
    let instance_url = require_non_empty(
        instance_url,
        "GitHub host login instance URL",
    )?;
    let login = normalized_optional_string(login);
    let cache_key = github_host_login_cache_key(&instance_url, login.as_deref());

    {
        let cache = github_host_login_header_cache()
            .lock()
            .map_err(|_| io::Error::other("GitHub host login cache lock was poisoned"))?;
        if let Some(auth_header) = cache.get(&cache_key) {
            return Ok(auth_header.clone());
        }
    }

    let (username, password) = request_github_host_login_credentials(
        &instance_url,
        login.as_deref(),
    )?;
    let auth_header = build_basic_authorization_header(&username, &password);

    github_host_login_header_cache()
        .lock()
        .map_err(|_| io::Error::other("GitHub host login cache lock was poisoned"))?
        .insert(cache_key, auth_header.clone());

    Ok(auth_header)
}

fn github_host_login_header_cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn github_host_login_cache_key(instance_url: &str, login: Option<&str>) -> String {
    let mut key = instance_url.trim().to_ascii_lowercase();
    key.push('\n');
    if let Some(login) = login {
        key.push_str(&login.trim().to_ascii_lowercase());
    }
    key
}

fn request_github_host_login_credentials(
    instance_url: &str,
    login: Option<&str>,
) -> io::Result<(String, String)> {
    let (protocol, host) = credential_context_from_instance_url(instance_url)?;
    let (preview, mut command) = prepare_github_credential_fill_command();

    let mut child = command.spawn().map_err(|error| {
        io::Error::other(format!("spawn git {preview}: {error}"))
    })?;
    let input = git_credential_fill_input(&protocol, &host, login);
    {
        use std::io::Write as _;

        let Some(mut stdin) = child.stdin.take() else {
            return Err(io::Error::other(
                "git credential fill did not expose stdin for credential input",
            ));
        };
        stdin.write_all(input.as_bytes()).map_err(|error| {
            io::Error::other(format!(
                "write git credential fill input for {instance_url:?}: {error}"
            ))
        })?;
    }

    let output = child.wait_with_output().map_err(|error| {
        io::Error::other(format!("wait for git {preview}: {error}"))
    })?;
    if !output.status.success() {
        return Err(io::Error::other(format_git_command_failure(
            &preview,
            &output,
        )));
    }

    parse_git_credential_fill_output(&String::from_utf8_lossy(&output.stdout))
}

fn prepare_github_credential_fill_command() -> (String, Command) {
    let preview = format!(
        "-c {GIT_CREDENTIAL_HELPER_RESET} -c credential.helper={GIT_CREDENTIAL_MANAGER_HELPER} credential fill"
    );
    let mut command = git_command();
    command
        .arg("-c")
        .arg(GIT_CREDENTIAL_HELPER_RESET)
        .arg("-c")
        .arg(format!(
            "credential.helper={GIT_CREDENTIAL_MANAGER_HELPER}"
        ))
        .arg("credential")
        .arg("fill")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_non_interactive_git_command(&mut command);

    (preview, command)
}

fn credential_context_from_instance_url(instance_url: &str) -> io::Result<(String, String)> {
    let trimmed = require_non_empty(
        instance_url,
        "GitHub host login instance URL",
    )?;
    let (protocol, remainder) = if let Some(rest) = trimmed.strip_prefix("https://") {
        ("https", rest)
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        ("http", rest)
    } else {
        ("https", trimmed.as_str())
    };
    let host = remainder
        .split('/')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "GitHub host login instance URL {trimmed:?} does not contain a host"
                ),
            )
        })?;

    Ok((String::from(protocol), String::from(host)))
}

fn git_credential_fill_input(protocol: &str, host: &str, login: Option<&str>) -> String {
    let mut input = format!("protocol={protocol}\nhost={host}\n");
    if let Some(login) = login {
        input.push_str("username=");
        input.push_str(login);
        input.push('\n');
    }
    input.push('\n');
    input
}

fn parse_git_credential_fill_output(output: &str) -> io::Result<(String, String)> {
    let mut username = None;
    let mut password = None;

    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("username=") {
            username = Some(require_non_empty(value, "Git credential username")?);
        } else if let Some(value) = trimmed.strip_prefix("password=") {
            password = Some(require_non_empty(value, "Git credential password")?);
        }
    }

    let username = username.ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidData,
            "git credential fill output is missing username",
        )
    })?;
    let password = password.ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidData,
            "git credential fill output is missing password",
        )
    })?;

    Ok((username, password))
}

fn build_basic_authorization_header(username: &str, password: &str) -> String {
    let token = BASE64_STANDARD.encode(format!("{username}:{password}"));
    format!("Authorization: Basic {token}")
}

fn normalized_optional_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn normalize_repository_url(repository_url: &str) -> io::Result<NormalizedRepositoryUrl> {
    let trimmed = require_non_empty(repository_url, "repository url")?;
    let (scheme, remainder) = trimmed.split_once("://").ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "repository url must use http:// or https://",
        )
    })?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "repository url must use http:// or https://",
        ));
    }

    let (host, raw_path) = remainder.split_once('/').ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "repository url must include a host and repository path",
        )
    })?;
    let host = require_non_empty(host, "repository host")?.to_ascii_lowercase();
    let path = raw_path
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .trim_matches('/');
    if path.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "repository url must include a repository path",
        ));
    }

    let segments = path
        .split('/')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let requires_owner_and_repository = matches!(
        host.as_str(),
        "github.com" | "gitlab.com" | "bitbucket.org"
    );
    if requires_owner_and_repository && segments.len() < 2 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "repository url must include owner and repository segments",
        ));
    }

    let scheme = scheme.to_ascii_lowercase();
    let normalized_path = segments.join("/");
    Ok(NormalizedRepositoryUrl {
        normalized_url: format!("{scheme}://{host}/{normalized_path}"),
        host: host.clone(),
        instance_url: format!("{scheme}://{host}"),
    })
}

fn detect_repository_provider(
    normalized: &NormalizedRepositoryUrl,
) -> DetectedRepositoryProvider {
    let (provider_id, provider_label, supports_interactive_login) =
        if normalized.host.eq_ignore_ascii_case("github.com") {
            (REPOSITORY_PROVIDER_GITHUB, "GitHub", true)
        } else if normalized.host.eq_ignore_ascii_case("gitlab.com") {
            (REPOSITORY_PROVIDER_GITLAB, "GitLab", false)
        } else if normalized.host.eq_ignore_ascii_case("bitbucket.org") {
            (REPOSITORY_PROVIDER_BITBUCKET, "Bitbucket", false)
        } else {
            (REPOSITORY_PROVIDER_UNKNOWN, "Unknown", false)
        };

    DetectedRepositoryProvider {
        provider_id,
        provider_label,
        supports_interactive_login,
        instance_url: normalized.instance_url.clone(),
    }
}

fn probe_repository_anonymously(repository_url: &str) -> io::Result<()> {
    let repository_url = require_non_empty(repository_url, "repository url")?;
    let _ = run_git_command_with_output(
        None,
        vec![
            String::from("ls-remote"),
            String::from("--symref"),
            repository_url,
            String::from("HEAD"),
        ],
        None,
        false,
    )?;

    Ok(())
}

fn classify_anonymous_probe_failure(error: &io::Error) -> AnonymousProbeClassification {
    let message = error.to_string().to_ascii_lowercase();

    let auth_indicators = [
        "authentication failed",
        "access denied",
        "terminal prompts disabled",
        "could not read username",
        "http basic",
        " 401",
        " 403",
        "403 forbidden",
        "401 unauthorized",
    ];
    if auth_indicators.iter().any(|indicator| message.contains(indicator)) {
        return AnonymousProbeClassification::AuthRequired;
    }

    let invalid_indicators = [
        "repository not found",
        "not found",
        "does not appear to be a git repository",
        "not a git repository",
        "requested url returned error: 404",
        "unable to access",
        "fatal: '/",
        "no such file or directory",
    ];
    if invalid_indicators
        .iter()
        .any(|indicator| message.contains(indicator))
    {
        return AnonymousProbeClassification::Invalid;
    }

    AnonymousProbeClassification::Unknown
}

/// Defines the repository snapshot that must be materialized into one local workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitWorkspaceSyncRequest {
    pub repository_url: String,
    pub workspace_path: PathBuf,
    pub git_tag: String,
    pub auth: GitAuthOptions,
}

/// Defines the repository ref that must be materialized into one local workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitWorkspaceSyncRefRequest {
    pub repository_url: String,
    pub workspace_path: PathBuf,
    pub git_ref: String,
    pub auth: GitAuthOptions,
}

/// Describes one tag discovered from a Git remote in ascending repository order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitTag {
    pub name: String,
    pub commit: String,
}

/// Defines the repository whose tags must be listed through the local Git CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitTagListRequest {
    pub repository_url: String,
    pub auth: GitAuthOptions,
}

/// Lists repository tags through the local Git CLI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitTagLister;

impl GitTagLister {
    /// Creates the default Git CLI-backed tag lister.
    pub const fn new() -> Self {
        Self
    }

    /// Lists remote tags in ascending version/refname order.
    pub fn list_tags(&self, request: &GitTagListRequest) -> io::Result<Vec<GitTag>> {
        let repository_url = require_non_empty(&request.repository_url, "repository url")?;
        let output = run_git_command_with_output(
            None,
            request.auth.append_git_args([
                "ls-remote",
                "--refs",
                "--tags",
                "--sort=version:refname",
                repository_url.as_str(),
            ]),
            request.auth.credential_helper.as_deref(),
            request.auth.preserve_credential_helper,
        )?;

        parse_git_tags(&output)
    }
}

/// Materializes one repository tag into a deterministic local workspace directory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitWorkspaceSyncer;

impl GitWorkspaceSyncer {
    /// Creates the default Git CLI-backed workspace syncer.
    pub const fn new() -> Self {
        Self
    }

    /// Clones or refreshes one local workspace and checks out the requested tag in detached HEAD state.
    pub fn sync_tag(&self, request: &GitWorkspaceSyncRequest) -> io::Result<()> {
        let mut reporter = NoopGitProgressReporter;
        self.sync_tag_with_progress(request, &mut reporter)
    }

    /// Clones or refreshes one local workspace and checks out the requested tag in detached HEAD state.
    pub fn sync_tag_with_progress(
        &self,
        request: &GitWorkspaceSyncRequest,
        reporter: &mut dyn GitProgressReporter,
    ) -> io::Result<()> {
        self.sync_ref_with_progress(&GitWorkspaceSyncRefRequest {
            repository_url: request.repository_url.clone(),
            workspace_path: request.workspace_path.clone(),
            git_ref: request.git_tag.clone(),
            auth: request.auth.clone(),
        }, reporter)
    }

    /// Clones or refreshes one local workspace and checks out the requested ref in detached HEAD state.
    pub fn sync_ref(&self, request: &GitWorkspaceSyncRefRequest) -> io::Result<()> {
        let mut reporter = NoopGitProgressReporter;
        self.sync_ref_with_progress(request, &mut reporter)
    }

    /// Clones or refreshes one local workspace and checks out the requested ref in detached HEAD state.
    pub fn sync_ref_with_progress(
        &self,
        request: &GitWorkspaceSyncRefRequest,
        reporter: &mut dyn GitProgressReporter,
    ) -> io::Result<()> {
        let repository_url = require_non_empty(&request.repository_url, "repository url")?;
        let git_ref = require_non_empty(&request.git_ref, "git ref")?;
        let workspace_path = normalize_workspace_path(&request.workspace_path)?;
        let workspace_display = workspace_path.display().to_string();

        reporter.report(&format!(
            "Preparing checkout workspace at '{}'.",
            workspace_display,
        ));

        if let Some(parent) = workspace_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let git_dir = workspace_path.join(".git");
        if !git_dir.exists() {
            reporter.report(&format!(
                "Cloning repository metadata into '{}'.",
                workspace_display,
            ));
            reset_workspace_path(&workspace_path)?;
            let clone_destination = workspace_path.display().to_string();
            run_git_command(
                None,
                request.auth.append_git_args([
                    "clone",
                    "--no-checkout",
                    repository_url.as_str(),
                    clone_destination.as_str(),
                ]),
                request.auth.credential_helper.as_deref(),
                request.auth.preserve_credential_helper,
            )
            .map_err(|error| io::Error::other(format!(
                "clone repository into workspace: {error}"
            )))?;
        } else {
            reporter.report(&format!(
                "Refreshing Git remote configuration for existing workspace '{}'.",
                workspace_display,
            ));
            run_git_command(
                Some(&workspace_path),
                vec![
                    String::from("remote"),
                    String::from("set-url"),
                    String::from("origin"),
                    repository_url,
                ],
                None,
                false,
            )
            .map_err(|error| io::Error::other(format!(
                "set workspace remote origin: {error}"
            )))?;
        }

        reporter.report(&format!(
            "Ensuring Git long path support for '{}'.",
            workspace_display,
        ));
        ensure_workspace_long_paths(&workspace_path).map_err(|error| {
            io::Error::other(format!("configure workspace long path support: {error}"))
        })?;

        reporter.report(&format!(
            "Fetching ref '{}' from origin into '{}'.",
            git_ref,
            workspace_display,
        ));
        run_git_command(
            Some(&workspace_path),
            request.auth.append_git_args([
                "fetch",
                "--force",
                "--depth=1",
                "origin",
                git_ref.as_str(),
            ]),
            request.auth.credential_helper.as_deref(),
            request.auth.preserve_credential_helper,
        )
        .map_err(|error| io::Error::other(format!(
            "fetch repository ref {git_ref:?}: {error}"
        )))?;

        reporter.report(&format!(
            "Checking out fetched ref '{}' in detached HEAD mode.",
            git_ref,
        ));
        run_git_command(
            Some(&workspace_path),
            vec![
                String::from("checkout"),
                String::from("--detach"),
                String::from("--force"),
                String::from("FETCH_HEAD"),
            ],
            None,
            false,
        )
        .map_err(|error| io::Error::other(format!(
            "checkout repository ref {git_ref:?}: {error}"
        )))?;

        reporter.report(&format!(
            "Cleaning untracked files in '{}'.",
            workspace_display,
        ));
        run_git_command(
            Some(&workspace_path),
            vec![String::from("clean"), String::from("-fdx")],
            None,
            false,
        )
        .map_err(|error| io::Error::other(format!(
            "clean workspace after checkout: {error}"
        )))?;

        reporter.report(&format!(
            "Repository ref '{}' is ready at '{}'.",
            git_ref,
            workspace_display,
        ));

        Ok(())
    }
}

fn require_non_empty(value: &str, label: &str) -> io::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("{label} must not be empty"),
        ));
    }

    Ok(trimmed.to_owned())
}

fn decode_auth_config<T>(config_json: &str) -> io::Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_str(config_json.trim()).map_err(|error| {
        io::Error::new(ErrorKind::InvalidData, format!("decode auth config: {error}"))
    })
}

fn normalize_workspace_path(workspace_path: &Path) -> io::Result<PathBuf> {
    if workspace_path.as_os_str().is_empty() || workspace_path == Path::new(".") {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "workspace path must not be empty",
        ));
    }

    Ok(workspace_path.to_path_buf())
}

fn reset_workspace_path(workspace_path: &Path) -> io::Result<()> {
    if !workspace_path.exists() {
        return Ok(());
    }

    let metadata = fs::metadata(workspace_path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(workspace_path)
    } else {
        fs::remove_file(workspace_path)
    }
}

fn run_git_command(
    working_dir: Option<&Path>,
    args: Vec<String>,
    credential_helper: Option<&str>,
    preserve_credential_helper: bool,
) -> io::Result<()> {
    let output = run_git_command_with_output(
        working_dir,
        args,
        credential_helper,
        preserve_credential_helper,
    )?;
    let _ = output;

    Ok(())
}

fn run_git_command_with_output(
    working_dir: Option<&Path>,
    args: Vec<String>,
    credential_helper: Option<&str>,
    preserve_credential_helper: bool,
) -> io::Result<String> {
    let (preview, mut command) =
        prepare_git_command(
            working_dir,
            args,
            credential_helper,
            preserve_credential_helper,
        );

    let output = command.output().map_err(|error| {
        io::Error::other(format!("spawn git {preview}: {error}"))
    })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }

    Err(io::Error::other(format_git_command_failure(
        &preview,
        &output,
    )))
}

fn prepare_git_command(
    working_dir: Option<&Path>,
    args: Vec<String>,
    credential_helper: Option<&str>,
    preserve_credential_helper: bool,
) -> (String, Command) {
    let args = platform_git_command_args(
        args,
        credential_helper,
        preserve_credential_helper,
    );
    let preview = args.join(" ");
    let mut command = git_command();
    command.args(args.iter().map(String::as_str));
    configure_non_interactive_git_command(&mut command);
    if let Some(working_dir) = working_dir {
        command.current_dir(working_dir);
    }

    (preview, command)
}

fn configure_non_interactive_git_command(command: &mut Command) {
    command
        .env(GIT_TERMINAL_PROMPT_ENV, GIT_TERMINAL_PROMPT_DISABLED)
    .env(GCM_INTERACTIVE_ENV, GCM_INTERACTIVE_NEVER)
    .env_remove(GIT_ASKPASS_ENV)
    .env_remove(SSH_ASKPASS_ENV);
}

fn git_command() -> Command {
    #[cfg(test)]
    if let Some(path) = std::env::var_os(TEST_GIT_EXECUTABLE_ENV) {
        return Command::new(path);
    }

    Command::new("git")
}

fn format_git_command_failure(preview: &str, output: &Output) -> String {
    let exit_detail = match output.status.code() {
        Some(code) => format!("exit code {code}"),
        None => String::from("termination by signal"),
    };
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = stderr.trim();
    let stdout = stdout.trim();

    let mut details = format!("git {preview} failed with {exit_detail}");
    if !stderr.is_empty() {
        details.push_str("; stderr: ");
        details.push_str(stderr);
    }
    if !stdout.is_empty() {
        details.push_str("; stdout: ");
        details.push_str(stdout);
    }

    details
}

fn ensure_workspace_long_paths(workspace_path: &Path) -> io::Result<()> {
    if !cfg!(windows) {
        return Ok(());
    }

    run_git_command(
        Some(workspace_path),
        vec![
            String::from("config"),
            String::from("core.longpaths"),
            String::from("true"),
        ],
        None,
        false,
    )
}

fn platform_git_command_args(
    args: Vec<String>,
    credential_helper: Option<&str>,
    preserve_credential_helper: bool,
) -> Vec<String> {
    let mut platform_args = Vec::new();
    platform_args.push(String::from("-c"));
    platform_args.push(String::from(GIT_CORE_ASKPASS_RESET));
    platform_args.push(String::from("-c"));
    platform_args.push(String::from(GIT_CREDENTIAL_INTERACTIVE_DISABLED));
    if let Some(credential_helper) = credential_helper {
        platform_args.push(String::from("-c"));
        platform_args.push(format!("credential.helper={credential_helper}"));
    } else if !preserve_credential_helper {
        platform_args.push(String::from("-c"));
        platform_args.push(String::from(GIT_CREDENTIAL_HELPER_RESET));
    }
    if cfg!(windows) {
        platform_args.push(String::from("-c"));
        platform_args.push(String::from("core.longpaths=true"));
    }
    platform_args.extend(args);
    platform_args
}

fn parse_git_tags(output: &str) -> io::Result<Vec<GitTag>> {
    let mut tags = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let mut fields = trimmed.split_whitespace();
        let commit = fields
            .next()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "git ls-remote output is missing commit"))?;
        let reference = fields
            .next()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "git ls-remote output is missing ref"))?;
        if fields.next().is_some() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!("git ls-remote output has unexpected extra fields: {trimmed}"),
            ));
        }

        let Some(tag_name) = reference.strip_prefix("refs/tags/") else {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!("git ls-remote output returned non-tag ref {reference:?}"),
            ));
        };

        tags.push(GitTag {
            name: tag_name.to_owned(),
            commit: commit.to_owned(),
        });
    }

    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::prepare_github_credential_fill_command;
    use super::{
        assess_repository_access,
        detect_repository_provider_from_url,
        format_git_command_failure, git_auth_options_from_credentials,
        git_auth_options_from_credentials_with_github_header_resolver,
        normalize_repository_url, detect_repository_provider,
        parse_git_credential_fill_output, platform_git_command_args,
        prepare_git_command, GitAuthOptions, GitTagListRequest,
        GitTagLister, GitWorkspaceSyncRefRequest, GitWorkspaceSyncRequest,
        GitWorkspaceSyncer, KIND_GIT_HTTP_BASIC, KIND_GIT_HTTP_BEARER,
        KIND_GIT_HTTP_GITHUB_HOST_LOGIN,
    };
    use std::ffi::OsStr;
    use std::fs;
    use std::io::{self, BufRead, BufReader, ErrorKind, Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output, Stdio};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::{self, JoinHandle};

    #[test]
    fn git_auth_options_from_basic_credentials_builds_authorization_header() {
        let auth = git_auth_options_from_credentials(
            KIND_GIT_HTTP_BASIC,
            r#"{"username":"comrade","password":"sickle"}"#,
        )
        .expect("basic credentials should parse");

        assert_eq!(
            auth,
            GitAuthOptions {
                extra_headers: vec![String::from("Authorization: Basic Y29tcmFkZTpzaWNrbGU=")],
                credential_helper: None,
                preserve_credential_helper: false,
            }
        );
    }

    #[test]
    fn git_auth_options_from_bearer_credentials_builds_authorization_header() {
        let auth = git_auth_options_from_credentials(
            KIND_GIT_HTTP_BEARER,
            r#"{"token":"red-banner"}"#,
        )
        .expect("bearer credentials should parse");

        assert_eq!(
            auth,
            GitAuthOptions {
                extra_headers: vec![String::from("Authorization: Bearer red-banner")],
                credential_helper: None,
                preserve_credential_helper: false,
            }
        );
    }

    #[test]
    fn git_auth_options_from_github_host_login_resolves_authorization_header_once() {
        let auth = git_auth_options_from_credentials_with_github_header_resolver(
            KIND_GIT_HTTP_GITHUB_HOST_LOGIN,
            r#"{"provider":"github","instance_url":"https://github.com","login":"worker@collective"}"#,
            |instance_url, login| {
                assert_eq!(instance_url, "https://github.com");
                assert_eq!(login, Some("worker@collective"));
                Ok(String::from("Authorization: Basic c3RyaWtlOnJlZC1iYW5uZXI="))
            },
        )
        .expect("GitHub host login credentials should parse");

        assert_eq!(
            auth,
            GitAuthOptions {
                extra_headers: vec![String::from(
                    "Authorization: Basic c3RyaWtlOnJlZC1iYW5uZXI=",
                )],
                credential_helper: None,
                preserve_credential_helper: false,
            }
        );
    }

    #[test]
    fn parse_git_credential_fill_output_reads_username_and_password() {
        let (username, password) = parse_git_credential_fill_output(
            "protocol=https\nhost=github.com\nusername=strike\npassword=red-banner\n",
        )
        .expect("credential fill output should parse");

        assert_eq!(username, "strike");
        assert_eq!(password, "red-banner");
    }

    #[test]
    fn git_auth_options_rejects_unknown_kinds() {
        let error = git_auth_options_from_credentials("ssh", r#"{}"#)
            .expect_err("unsupported kind should fail");
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn git_auth_options_rejects_missing_required_fields() {
        let error = git_auth_options_from_credentials(
            KIND_GIT_HTTP_BASIC,
            r#"{"username":"","password":"secret"}"#,
        )
        .expect_err("blank username should fail");
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn detect_repository_provider_classifies_supported_hosts() {
        let github = normalize_repository_url("https://github.com/indiegabo/hgp.git")
            .expect("GitHub url should normalize");
        let gitlab = normalize_repository_url("https://gitlab.com/collective/hgp.git")
            .expect("GitLab url should normalize");
        let bitbucket =
            normalize_repository_url("https://bitbucket.org/collective/hgp.git")
                .expect("Bitbucket url should normalize");
        let unknown = normalize_repository_url("https://forge.example.com/collective/hgp.git")
            .expect("unknown host url should normalize");

        assert_eq!(detect_repository_provider(&github).provider_id, "github");
        assert_eq!(detect_repository_provider(&gitlab).provider_id, "gitlab");
        assert_eq!(detect_repository_provider(&bitbucket).provider_id, "bitbucket");
        assert_eq!(detect_repository_provider(&unknown).provider_id, "unknown");
    }

    #[test]
    fn detect_repository_provider_from_url_returns_normalized_provider_metadata() {
        let detection = detect_repository_provider_from_url(
            "https://github.com/indiegabo/hgp.git",
        )
        .expect("GitHub provider detection should succeed");

        assert_eq!(detection.provider_id, "github");
        assert_eq!(detection.provider_label, "GitHub");
        assert_eq!(detection.instance_url, "https://github.com");
        assert_eq!(
            detection.normalized_url,
            "https://github.com/indiegabo/hgp.git",
        );
        assert!(detection.supports_interactive_login);
    }

    #[test]
    fn assess_repository_access_reports_public_repository_without_auth() {
        let root = test_root("access-assessment-public");
        if root.exists() {
            fs::remove_dir_all(&root).expect("existing public assessment root should be removable");
        }

        let project_root = root.join("http-root");
        create_bare_repository_with_tags(
            &project_root,
            "public-tags.git",
            "2022.3.20f1",
            &["v1.0.0"],
        );
        let server = AuthenticatedGitHttpServer::start_public(&project_root);

        let assessment = assess_repository_access(&server.repository_url("public-tags.git"))
            .expect("public repository assessment should succeed");

        assert_eq!(assessment.provider_id, "unknown");
        assert_eq!(assessment.visibility, "public");
        assert_eq!(assessment.auth_requirement, "none");
        assert_eq!(assessment.auth_status, "not_required");
        assert!(!assessment.supports_interactive_login);

        drop(server);
        fs::remove_dir_all(root).expect("temporary git test root should be removable");
    }

    #[test]
    fn assess_repository_access_reports_private_repository_when_auth_is_required() {
        let root = test_root("access-assessment-private");
        if root.exists() {
            fs::remove_dir_all(&root).expect("existing private assessment root should be removable");
        }

        let project_root = root.join("http-root");
        create_bare_repository_with_tags(
            &project_root,
            "private-tags.git",
            "2022.3.20f1",
            &["v2.0.0"],
        );
        let auth = git_auth_options_from_credentials(
            KIND_GIT_HTTP_BASIC,
            r#"{"username":"comrade","password":"sickle"}"#,
        )
        .expect("basic credentials should parse");
        let server = AuthenticatedGitHttpServer::start(
            &project_root,
            authorization_header_value(&auth),
        );

        let assessment = assess_repository_access(&server.repository_url("private-tags.git"))
            .expect("private repository assessment should classify auth requirement");

        assert_eq!(assessment.provider_id, "unknown");
        assert_eq!(assessment.visibility, "private");
        assert_eq!(assessment.auth_requirement, "required");
        assert_eq!(assessment.auth_status, "unsupported");

        drop(server);
        fs::remove_dir_all(root).expect("temporary git test root should be removable");
    }

    #[test]
    fn assess_repository_access_reports_invalid_repository_for_missing_remote() {
        let root = test_root("access-assessment-invalid");
        if root.exists() {
            fs::remove_dir_all(&root).expect("existing invalid assessment root should be removable");
        }

        let project_root = root.join("http-root");
        fs::create_dir_all(&project_root).expect("http root should create");
        let server = AuthenticatedGitHttpServer::start_public(&project_root);

        let assessment = assess_repository_access(&server.repository_url("missing.git"))
            .expect("invalid repository assessment should classify without raising shell error");

        assert_eq!(assessment.visibility, "invalid");
        assert_eq!(assessment.auth_requirement, "unknown");
        assert_eq!(assessment.auth_status, "unknown");

        drop(server);
        fs::remove_dir_all(root).expect("temporary git test root should be removable");
    }

    #[test]
    fn git_tag_lister_lists_repository_tags_in_ascending_order() {
        let root = test_root("list-tags");
        let repository_path = root.join("repo");
        create_repository_with_tags(
            &repository_path,
            "2022.3.20f1",
            &["v1.0.0", "v1.2.0", "v1.10.0"],
        );

        let tags = GitTagLister::new()
            .list_tags(&GitTagListRequest {
                repository_url: repository_path.display().to_string(),
                auth: GitAuthOptions::default(),
            })
            .expect("git tags should list");

        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0].name, "v1.0.0");
        assert_eq!(tags[1].name, "v1.2.0");
        assert_eq!(tags[2].name, "v1.10.0");
        assert!(tags.iter().all(|tag| !tag.commit.trim().is_empty()));

        fs::remove_dir_all(root).expect("temporary git test root should be removable");
    }

    #[test]
    fn workspace_sync_clone_failure_includes_exit_code_and_stderr() {
        let root = test_root("clone-failure-diagnostics");
        let workspace_path = root.join("workspace");
        let missing_repository_path = root.join("missing-repository");

        let error = GitWorkspaceSyncer::new()
            .sync_tag(&GitWorkspaceSyncRequest {
                repository_url: missing_repository_path.display().to_string(),
                workspace_path,
                git_tag: String::from("v1.0.0"),
                auth: GitAuthOptions::default(),
            })
            .expect_err("clone diagnostics should surface git failure details");

        let message = error.to_string();
        assert!(message.contains("clone repository into workspace"));
        assert!(message.contains("exit code"));
        assert!(message.contains("stderr:"));

        if root.exists() {
            fs::remove_dir_all(root).expect("temporary git test root should be removable");
        }
    }

    #[test]
    fn workspace_sync_fetch_failure_includes_exit_code_and_stderr() {
        let root = test_root("fetch-failure-diagnostics");
        let repository_path = root.join("repo");
        let workspace_path = root.join("workspace");
        create_repository_with_tags(&repository_path, "2022.3.20f1", &["v1.0.0"]);

        GitWorkspaceSyncer::new()
            .sync_tag(&GitWorkspaceSyncRequest {
                repository_url: repository_path.display().to_string(),
                workspace_path: workspace_path.clone(),
                git_tag: String::from("v1.0.0"),
                auth: GitAuthOptions::default(),
            })
            .expect("initial workspace sync should succeed");

        let error = GitWorkspaceSyncer::new()
            .sync_tag(&GitWorkspaceSyncRequest {
                repository_url: repository_path.display().to_string(),
                workspace_path,
                git_tag: String::from("v9.9.9"),
                auth: GitAuthOptions::default(),
            })
            .expect_err("fetch diagnostics should surface git failure details");

        let message = error.to_string();
        assert!(message.contains("fetch repository ref \"v9.9.9\""));
        assert!(message.contains("exit code"));
        assert!(message.contains("stderr:"));

        fs::remove_dir_all(root).expect("temporary git test root should be removable");
    }

    #[test]
    fn git_tag_lister_lists_private_repository_tags_with_basic_auth() {
        let root = test_root("private-tag-list");
        if root.exists() {
            fs::remove_dir_all(&root).expect("existing private git test root should be removable");
        }

        let project_root = root.join("http-root");
        create_bare_repository_with_tags(
            &project_root,
            "private-tags.git",
            "2022.3.20f1",
            &["v2.0.0", "v2.1.0"],
        );
        let auth = git_auth_options_from_credentials(
            KIND_GIT_HTTP_BASIC,
            r#"{"username":"comrade","password":"sickle"}"#,
        )
        .expect("basic credentials should parse");
        let server = AuthenticatedGitHttpServer::start(
            &project_root,
            authorization_header_value(&auth),
        );

        let tags = GitTagLister::new()
            .list_tags(&GitTagListRequest {
                repository_url: server.repository_url("private-tags.git"),
                auth,
            })
            .expect("private git tags should list with valid credentials");

        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].name, "v2.0.0");
        assert_eq!(tags[1].name, "v2.1.0");

        drop(server);
        fs::remove_dir_all(root).expect("temporary git test root should be removable");
    }

    #[test]
    fn git_tag_lister_reports_private_repository_credential_failures() {
        let root = test_root("private-tag-list-auth-failure");
        if root.exists() {
            fs::remove_dir_all(&root).expect("existing private git test root should be removable");
        }

        let project_root = root.join("http-root");
        create_bare_repository_with_tags(
            &project_root,
            "private-failure.git",
            "2022.3.20f1",
            &["v3.0.0"],
        );
        let required_auth = git_auth_options_from_credentials(
            KIND_GIT_HTTP_BASIC,
            r#"{"username":"comrade","password":"sickle"}"#,
        )
        .expect("basic credentials should parse");
        let server = AuthenticatedGitHttpServer::start(
            &project_root,
            authorization_header_value(&required_auth),
        );

        let error = GitTagLister::new()
            .list_tags(&GitTagListRequest {
                repository_url: server.repository_url("private-failure.git"),
                auth: GitAuthOptions::default(),
            })
            .expect_err("private git tags should fail without credentials");

        let message = error.to_string();
        assert!(message.contains("403"));
        assert!(message.contains("stderr:"));

        drop(server);
        fs::remove_dir_all(root).expect("temporary git test root should be removable");
    }

    #[test]
    fn workspace_syncer_syncs_private_repository_with_basic_auth() {
        let root = test_root("private-workspace-sync");
        if root.exists() {
            fs::remove_dir_all(&root).expect("existing private git test root should be removable");
        }

        let project_root = root.join("http-root");
        create_bare_repository_with_tags(
            &project_root,
            "private-workspace.git",
            "2022.3.20f1",
            &["v4.0.0"],
        );
        let auth = git_auth_options_from_credentials(
            KIND_GIT_HTTP_BASIC,
            r#"{"username":"comrade","password":"sickle"}"#,
        )
        .expect("basic credentials should parse");
        let server = AuthenticatedGitHttpServer::start(
            &project_root,
            authorization_header_value(&auth),
        );
        let workspace_path = root.join("workspace");

        GitWorkspaceSyncer::new()
            .sync_tag(&GitWorkspaceSyncRequest {
                repository_url: server.repository_url("private-workspace.git"),
                workspace_path: workspace_path.clone(),
                git_tag: String::from("v4.0.0"),
                auth,
            })
            .expect("private workspace sync should succeed with valid credentials");

        assert!(workspace_path.join(".git").is_dir());
        assert!(workspace_path
            .join("ProjectSettings")
            .join("ProjectVersion.txt")
            .is_file());

        drop(server);
        fs::remove_dir_all(root).expect("temporary git test root should be removable");
    }

    #[test]
    fn workspace_syncer_syncs_branch_ref() {
        let root = test_root("branch-workspace-sync");
        let repository_path = root.join("repo");
        let workspace_path = root.join("workspace");
        create_repository_with_tags(&repository_path, "2022.3.20f1", &["v1.0.0"]);
        let branch_name = current_branch_name(&repository_path);

        GitWorkspaceSyncer::new()
            .sync_ref(&GitWorkspaceSyncRefRequest {
                repository_url: repository_path.display().to_string(),
                workspace_path: workspace_path.clone(),
                git_ref: branch_name,
                auth: GitAuthOptions::default(),
            })
            .expect("branch workspace sync should succeed");

        assert!(workspace_path.join(".git").is_dir());
        assert!(workspace_path
            .join("ProjectSettings")
            .join("ProjectVersion.txt")
            .is_file());

        fs::remove_dir_all(root).expect("temporary git test root should be removable");
    }

    #[test]
    fn platform_git_command_args_enable_core_longpaths_on_windows() {
        let args = platform_git_command_args(
            vec![String::from("clean"), String::from("-fdx")],
            None,
            false,
        );

        assert_eq!(args[0], "-c");
        assert_eq!(args[1], "core.askPass=");
        assert_eq!(args[2], "-c");
        assert_eq!(args[3], "credential.interactive=false");
        assert_eq!(args[4], "-c");
        assert_eq!(args[5], "credential.helper=");
        if cfg!(windows) {
            assert_eq!(args[6], "-c");
            assert_eq!(args[7], "core.longpaths=true");
            assert_eq!(args[8], "clean");
            assert_eq!(args[9], "-fdx");
        } else {
            assert_eq!(args[6], "clean");
            assert_eq!(args[7], "-fdx");
        }
    }

    #[test]
    fn platform_git_command_args_can_override_credential_helper() {
        let args = platform_git_command_args(
            vec![String::from("ls-remote")],
            Some("manager"),
            false,
        );

        assert_eq!(args[0], "-c");
        assert_eq!(args[1], "core.askPass=");
        assert_eq!(args[2], "-c");
        assert_eq!(args[3], "credential.interactive=false");
        assert_eq!(args[4], "-c");
        assert_eq!(args[5], "credential.helper=manager");
        if cfg!(windows) {
            assert_eq!(args[6], "-c");
            assert_eq!(args[7], "core.longpaths=true");
            assert_eq!(args[8], "ls-remote");
        } else {
            assert_eq!(args[6], "ls-remote");
        }
    }

    #[test]
    fn prepare_git_command_disables_interactive_authentication() {
        let (_, command) = prepare_git_command(
            None,
            vec![String::from("ls-remote")],
            None,
            false,
        );

        assert_eq!(
            command_env_value(&command, "GIT_TERMINAL_PROMPT").as_deref(),
            Some("0")
        );
        assert_eq!(
            command_env_value(&command, "GCM_INTERACTIVE").as_deref(),
            Some("never")
        );
        assert!(command_env_is_removed(&command, "GIT_ASKPASS"));
        assert!(command_env_is_removed(&command, "SSH_ASKPASS"));
    }

    #[test]
    fn prepare_github_credential_fill_command_disables_interactive_authentication() {
        let (_, command) = prepare_github_credential_fill_command();

        assert_eq!(
            command_env_value(&command, "GIT_TERMINAL_PROMPT").as_deref(),
            Some("0")
        );
        assert_eq!(
            command_env_value(&command, "GCM_INTERACTIVE").as_deref(),
            Some("never")
        );
        assert!(command_env_is_removed(&command, "GIT_ASKPASS"));
        assert!(command_env_is_removed(&command, "SSH_ASKPASS"));
    }

    #[test]
    fn prepare_git_command_can_set_manager_helper() {
        let (_, command) = prepare_git_command(
            None,
            vec![String::from("ls-remote")],
            Some("manager"),
            false,
        );

        let helper_override_present = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .any(|argument| argument == "credential.helper=manager");

        assert!(helper_override_present);
    }

    fn command_env_value(command: &Command, key: &str) -> Option<String> {
        command.get_envs().find_map(|(name, value)| {
            if name == OsStr::new(key) {
                value.map(|resolved| resolved.to_string_lossy().into_owned())
            } else {
                None
            }
        })
    }

    fn command_env_is_removed(command: &Command, key: &str) -> bool {
        command
            .get_envs()
            .any(|(name, value)| name == OsStr::new(key) && value.is_none())
    }

    #[derive(Debug)]
    struct AuthenticatedGitHttpServer {
        address: SocketAddr,
        shutdown: Arc<AtomicBool>,
        join_handle: Option<JoinHandle<()>>,
    }

    impl AuthenticatedGitHttpServer {
        fn start(project_root: &Path, expected_authorization_value: String) -> Self {
            Self::start_with_optional_auth(
                project_root,
                Some(expected_authorization_value),
            )
        }

        fn start_public(project_root: &Path) -> Self {
            Self::start_with_optional_auth(project_root, None)
        }

        fn start_with_optional_auth(
            project_root: &Path,
            expected_authorization_value: Option<String>,
        ) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0")
                .expect("git http test listener should bind");
            let address = listener
                .local_addr()
                .expect("git http test listener should expose an address");
            let shutdown = Arc::new(AtomicBool::new(false));
            let shutdown_flag = shutdown.clone();
            let config = GitHttpServerConfig {
                git_http_backend_path: git_http_backend_path(),
                project_root: project_root.to_path_buf(),
                expected_authorization_value,
            };
            let join_handle = thread::spawn(move || loop {
                let (mut stream, _) = listener
                    .accept()
                    .expect("git http test server should accept connections");
                if shutdown_flag.load(Ordering::Relaxed) {
                    break;
                }

                handle_git_http_connection(&mut stream, &config)
                    .expect("git http test server should handle one connection");
            });

            Self {
                address,
                shutdown,
                join_handle: Some(join_handle),
            }
        }

        fn repository_url(&self, repository_name: &str) -> String {
            format!("http://{}/{repository_name}", self.address)
        }

        fn shutdown(&mut self) {
            if self.shutdown.swap(true, Ordering::Relaxed) {
                return;
            }

            let _ = TcpStream::connect(self.address);
            if let Some(join_handle) = self.join_handle.take() {
                join_handle
                    .join()
                    .expect("git http test server thread should join cleanly");
            }
        }
    }

    impl Drop for AuthenticatedGitHttpServer {
        fn drop(&mut self) {
            self.shutdown();
        }
    }

    #[derive(Debug, Clone)]
    struct GitHttpServerConfig {
        git_http_backend_path: PathBuf,
        project_root: PathBuf,
        expected_authorization_value: Option<String>,
    }

    #[derive(Debug, Clone)]
    struct HttpRequest {
        method: String,
        target: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    impl HttpRequest {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers.iter().find_map(|(header_name, value)| {
                if header_name.eq_ignore_ascii_case(name) {
                    Some(value.as_str())
                } else {
                    None
                }
            })
        }
    }

    fn authorization_header_value(auth: &GitAuthOptions) -> String {
        auth.extra_headers
            .first()
            .and_then(|header| header.strip_prefix("Authorization: "))
            .expect("auth options should contain an authorization header")
            .to_owned()
    }

    fn create_bare_repository_with_tags(
        project_root: &Path,
        repository_name: &str,
        unity_version: &str,
        tags: &[&str],
    ) -> PathBuf {
        fs::create_dir_all(project_root).expect("git http project root should create");

        let source_repository_path = project_root.join(format!("{repository_name}-source"));
        let bare_repository_path = project_root.join(repository_name);
        create_repository_with_tags(&source_repository_path, unity_version, tags);
        if bare_repository_path.exists() {
            fs::remove_dir_all(&bare_repository_path)
                .expect("existing bare repository should be removable");
        }

        let source_repository = source_repository_path.display().to_string();
        let bare_repository = bare_repository_path.display().to_string();
        run_git_test_command(
            project_root,
            &[
                "clone",
                "--bare",
                source_repository.as_str(),
                bare_repository.as_str(),
            ],
        );

        bare_repository_path
    }

    fn handle_git_http_connection(
        stream: &mut TcpStream,
        config: &GitHttpServerConfig,
    ) -> io::Result<()> {
        let Some(request) = read_http_request(stream)? else {
            return Ok(());
        };

        if let Some(expected_authorization_value) =
            config.expected_authorization_value.as_deref()
        {
            let Some(authorization) = request.header("authorization") else {
                return write_forbidden_response(stream);
            };
            if authorization != expected_authorization_value {
                return write_forbidden_response(stream);
            }
        }

        let output = run_git_http_backend(config, &request)?;
        write_git_http_backend_response(stream, &output)
    }

    fn read_http_request(stream: &mut TcpStream) -> io::Result<Option<HttpRequest>> {
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut request_line = String::new();
        if reader.read_line(&mut request_line)? == 0 {
            return Ok(None);
        }

        let request_line = request_line.trim_end_matches(['\r', '\n']);
        if request_line.is_empty() {
            return Ok(None);
        }

        let mut parts = request_line.split_whitespace();
        let method = parts
            .next()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "request is missing method"))?
            .to_owned();
        let target = parts
            .next()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "request is missing target"))?
            .to_owned();
        let _version = parts
            .next()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "request is missing version"))?;
        if parts.next().is_some() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!("request line has unexpected extra fields: {request_line}"),
            ));
        }

        let mut headers = Vec::new();
        loop {
            let mut header_line = String::new();
            if reader.read_line(&mut header_line)? == 0 {
                return Err(io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "request headers ended unexpectedly",
                ));
            }

            let header_line = header_line.trim_end_matches(['\r', '\n']);
            if header_line.is_empty() {
                break;
            }

            let (name, value) = header_line.split_once(':').ok_or_else(|| {
                io::Error::new(
                    ErrorKind::InvalidData,
                    format!("request header is malformed: {header_line}"),
                )
            })?;
            headers.push((name.trim().to_owned(), value.trim().to_owned()));
        }

        let expect_continue = headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("expect")
                && value.eq_ignore_ascii_case("100-continue")
        });
        let content_length = headers
            .iter()
            .find_map(|(name, value)| {
                if !name.eq_ignore_ascii_case("content-length") {
                    return None;
                }

                Some(
                    value
                        .parse::<usize>()
                        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error)),
                )
            })
            .transpose()?
            .unwrap_or(0);
        if expect_continue && content_length > 0 {
            stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
            stream.flush()?;
        }

        let mut body = vec![0; content_length];
        if content_length > 0 {
            reader.read_exact(&mut body)?;
        }

        Ok(Some(HttpRequest {
            method,
            target,
            headers,
            body,
        }))
    }

    fn run_git_http_backend(
        config: &GitHttpServerConfig,
        request: &HttpRequest,
    ) -> io::Result<Output> {
        let (path_info, query_string) = split_request_target(&request.target);
        let mut command = Command::new(&config.git_http_backend_path);
        command
            .env("GIT_PROJECT_ROOT", &config.project_root)
            .env("GIT_HTTP_EXPORT_ALL", "1")
            .env("REQUEST_METHOD", &request.method)
            .env("PATH_INFO", path_info)
            .env("QUERY_STRING", query_string)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(content_type) = request.header("content-type") {
            command.env("CONTENT_TYPE", content_type);
        }
        if !request.body.is_empty() {
            command.env("CONTENT_LENGTH", request.body.len().to_string());
        }

        let mut child = command.spawn()?;
        if !request.body.is_empty() {
            child
                .stdin
                .as_mut()
                .expect("git http backend stdin should be available")
                .write_all(&request.body)?;
        }

        child.wait_with_output()
    }

    fn split_request_target(target: &str) -> (&str, &str) {
        target.split_once('?').unwrap_or((target, ""))
    }

    fn write_git_http_backend_response(
        stream: &mut TcpStream,
        output: &Output,
    ) -> io::Result<()> {
        if !output.status.success() {
            let body = format_git_command_failure("git-http-backend", output);
            return write_http_response(
                stream,
                "500 Internal Server Error",
                &[
                    (
                        String::from("Content-Type"),
                        String::from("text/plain; charset=utf-8"),
                    ),
                    (String::from("Content-Length"), body.len().to_string()),
                    (String::from("Connection"), String::from("close")),
                ],
                body.as_bytes(),
            );
        }

        let (header_bytes, body) = split_cgi_output(&output.stdout)?;
        let header_text = String::from_utf8_lossy(header_bytes);
        let mut status = String::from("200 OK");
        let mut headers = Vec::new();
        let mut has_content_length = false;

        for line in header_text.lines() {
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }

            let (name, value) = line.split_once(':').ok_or_else(|| {
                io::Error::new(
                    ErrorKind::InvalidData,
                    format!("git-http-backend emitted malformed header {line:?}"),
                )
            })?;
            let name = name.trim();
            let value = value.trim();
            if name.eq_ignore_ascii_case("status") {
                status = value.to_owned();
                continue;
            }
            if name.eq_ignore_ascii_case("content-length") {
                has_content_length = true;
            }

            headers.push((name.to_owned(), value.to_owned()));
        }
        if !has_content_length {
            headers.push((String::from("Content-Length"), body.len().to_string()));
        }
        headers.push((String::from("Connection"), String::from("close")));

        write_http_response(stream, &status, &headers, body)
    }

    fn split_cgi_output(output: &[u8]) -> io::Result<(&[u8], &[u8])> {
        if let Some(index) = find_subsequence(output, b"\r\n\r\n") {
            return Ok((&output[..index], &output[index + 4..]));
        }
        if let Some(index) = find_subsequence(output, b"\n\n") {
            return Ok((&output[..index], &output[index + 2..]));
        }

        Err(io::Error::new(
            ErrorKind::InvalidData,
            "git-http-backend output is missing a header separator",
        ))
    }

    fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    fn write_forbidden_response(stream: &mut TcpStream) -> io::Result<()> {
        write_http_response(
            stream,
            "403 Forbidden",
            &[
                (
                    String::from("Content-Type"),
                    String::from("text/plain; charset=utf-8"),
                ),
                (String::from("Content-Length"), String::from("9")),
                (String::from("Connection"), String::from("close")),
            ],
            b"forbidden",
        )
    }

    fn write_http_response(
        stream: &mut TcpStream,
        status: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> io::Result<()> {
        write!(stream, "HTTP/1.1 {status}\r\n")?;
        for (name, value) in headers {
            write!(stream, "{name}: {value}\r\n")?;
        }
        write!(stream, "\r\n")?;
        stream.write_all(body)?;
        stream.flush()
    }

    fn git_http_backend_path() -> PathBuf {
        let output = Command::new("git")
            .arg("--exec-path")
            .output()
            .expect("git --exec-path should spawn");
        if !output.status.success() {
            panic!(
                "git --exec-path failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }

        let exec_path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let executable = if cfg!(windows) {
            "git-http-backend.exe"
        } else {
            "git-http-backend"
        };

        PathBuf::from(exec_path).join(executable)
    }

    fn create_repository_with_tags(
        repository_path: &Path,
        unity_version: &str,
        tags: &[&str],
    ) {
        if repository_path.exists() {
            fs::remove_dir_all(repository_path)
                .expect("existing git test repository should be removable");
        }

        fs::create_dir_all(repository_path.join("ProjectSettings"))
            .expect("project settings directory should create");
        fs::write(
            repository_path.join("ProjectSettings").join("ProjectVersion.txt"),
            format!("m_EditorVersion: {unity_version}\n"),
        )
        .expect("project version file should write");

        run_git_test_command(repository_path, &["init"]);
        run_git_test_command(repository_path, &["config", "user.name", "runtime-git-tests"]);
        run_git_test_command(
            repository_path,
            &["config", "user.email", "runtime-git-tests@example.com"],
        );

        for (index, tag) in tags.iter().enumerate() {
            fs::write(
                repository_path.join("artifact.txt"),
                format!("artifact-{index}\n"),
            )
            .expect("artifact marker should write");
            run_git_test_command(repository_path, &["add", "."]);
            run_git_test_command(repository_path, &["commit", "-m", tag]);
            run_git_test_command(repository_path, &["tag", tag]);
        }
    }

    fn current_branch_name(repository_path: &Path) -> String {
        let output = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(repository_path)
            .output()
            .expect("git branch --show-current should spawn");
        if !output.status.success() {
            panic!(
                "git branch --show-current failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }

        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn run_git_test_command(working_dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(working_dir)
            .output()
            .expect("git test command should spawn");
        if output.status.success() {
            return;
        }

        panic!(
            "git {:?} failed: {}{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "handy-games-publisher-runtime-git-{label}-{}",
            std::process::id()
        ))
    }
}
