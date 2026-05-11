#![forbid(unsafe_code)]

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Identifies Git credentials backed by HTTP basic authentication.
pub const KIND_GIT_HTTP_BASIC: &str = "git-http-basic";

/// Identifies Git credentials backed by HTTP bearer authentication.
pub const KIND_GIT_HTTP_BEARER: &str = "git-http-bearer";

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

/// Resolves stored credentials into Git CLI authentication headers.
pub fn git_auth_options_from_credentials(
    kind: &str,
    config_json: &str,
) -> io::Result<GitAuthOptions> {
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
            })
        }
        _ => Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("unsupported credentials kind {kind:?}"),
        )),
    }
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
        self.sync_ref(&GitWorkspaceSyncRefRequest {
            repository_url: request.repository_url.clone(),
            workspace_path: request.workspace_path.clone(),
            git_ref: request.git_tag.clone(),
            auth: request.auth.clone(),
        })
    }

    /// Clones or refreshes one local workspace and checks out the requested ref in detached HEAD state.
    pub fn sync_ref(&self, request: &GitWorkspaceSyncRefRequest) -> io::Result<()> {
        let repository_url = require_non_empty(&request.repository_url, "repository url")?;
        let git_ref = require_non_empty(&request.git_ref, "git ref")?;
        let workspace_path = normalize_workspace_path(&request.workspace_path)?;

        if let Some(parent) = workspace_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let git_dir = workspace_path.join(".git");
        if !git_dir.exists() {
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
            )
            .map_err(|error| io::Error::other(format!(
                "clone repository into workspace: {error}"
            )))?;
        } else {
            run_git_command(
                Some(&workspace_path),
                vec![
                    String::from("remote"),
                    String::from("set-url"),
                    String::from("origin"),
                    repository_url,
                ],
            )
            .map_err(|error| io::Error::other(format!(
                "set workspace remote origin: {error}"
            )))?;
        }

        ensure_workspace_long_paths(&workspace_path).map_err(|error| {
            io::Error::other(format!("configure workspace long path support: {error}"))
        })?;

        run_git_command(
            Some(&workspace_path),
            request.auth.append_git_args([
                "fetch",
                "--force",
                "--depth=1",
                "origin",
                git_ref.as_str(),
            ]),
        )
        .map_err(|error| io::Error::other(format!(
            "fetch repository ref {git_ref:?}: {error}"
        )))?;

        run_git_command(
            Some(&workspace_path),
            vec![
                String::from("checkout"),
                String::from("--detach"),
                String::from("--force"),
                String::from("FETCH_HEAD"),
            ],
        )
        .map_err(|error| io::Error::other(format!(
            "checkout repository ref {git_ref:?}: {error}"
        )))?;

        run_git_command(
            Some(&workspace_path),
            vec![String::from("clean"), String::from("-fdx")],
        )
        .map_err(|error| io::Error::other(format!(
            "clean workspace after checkout: {error}"
        )))
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

fn run_git_command(working_dir: Option<&Path>, args: Vec<String>) -> io::Result<()> {
    let output = run_git_command_with_output(working_dir, args)?;
    let _ = output;

    Ok(())
}

fn run_git_command_with_output(working_dir: Option<&Path>, args: Vec<String>) -> io::Result<String> {
    let args = platform_git_command_args(args);
    let preview = args.join(" ");
    let mut command = Command::new("git");
    command.args(args.iter().map(String::as_str));
    if let Some(working_dir) = working_dir {
        command.current_dir(working_dir);
    }

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
    )
}

fn platform_git_command_args(args: Vec<String>) -> Vec<String> {
    if !cfg!(windows) {
        return args;
    }

    let mut platform_args = vec![String::from("-c"), String::from("core.longpaths=true")];
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
    use super::{
        format_git_command_failure, git_auth_options_from_credentials,
        platform_git_command_args, GitAuthOptions, GitTagListRequest, GitTagLister,
        GitWorkspaceSyncRefRequest, GitWorkspaceSyncRequest, GitWorkspaceSyncer,
        KIND_GIT_HTTP_BASIC, KIND_GIT_HTTP_BEARER,
    };
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
            }
        );
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
        let args = platform_git_command_args(vec![
            String::from("clean"),
            String::from("-fdx"),
        ]);

        if cfg!(windows) {
            assert_eq!(args[0], "-c");
            assert_eq!(args[1], "core.longpaths=true");
            assert_eq!(args[2], "clean");
            assert_eq!(args[3], "-fdx");
        } else {
            assert_eq!(args, vec![String::from("clean"), String::from("-fdx")]);
        }
    }

    #[derive(Debug)]
    struct AuthenticatedGitHttpServer {
        address: SocketAddr,
        shutdown: Arc<AtomicBool>,
        join_handle: Option<JoinHandle<()>>,
    }

    impl AuthenticatedGitHttpServer {
        fn start(project_root: &Path, expected_authorization_value: String) -> Self {
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
        expected_authorization_value: String,
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

        let Some(authorization) = request.header("authorization") else {
            return write_forbidden_response(stream);
        };
        if authorization != config.expected_authorization_value {
            return write_forbidden_response(stream);
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
            "handy-unity-builder-runtime-git-{label}-{}",
            std::process::id()
        ))
    }
}
