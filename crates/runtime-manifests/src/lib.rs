//! Loads pipeline manifests from disk, validates their schema, and synchronizes
//! repository automation definitions into the runtime database.

#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};

const API_VERSION: &str = "handy.games.publisher/v1alpha1";
const MANIFEST_KIND: &str = "Pipeline";
const SQLITE_BUSY_TIMEOUT_MILLIS: u64 = 5_000;
const DEFAULT_RUNNER_TYPE: &str = "host-native";
const DEFAULT_TIMEOUT_SECONDS: i64 = 3_600;
const DEFAULT_PUBLISH_KIND: &str = "filesystem";
const DEFAULT_BUILD_KIND: &str = "player";
const SUPPORTED_ENGINE_UNITY: &str = "unity";

/// Declares how the runtime scaffold interprets repository manifests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManifestCompatibility {
    FileSystem,
}

impl ManifestCompatibility {
    /// Returns the active manifest-loading contract for the scaffold.
    pub const fn description(self) -> &'static str {
        match self {
            Self::FileSystem => {
                "repository-root pipelines directory preserved during migration"
            }
        }
    }
}

/// Describes one invalid manifest file discovered during a directory scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadIssue {
    pub path: String,
    pub error: String,
}

/// Captures one manifest load round over a filesystem directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LoadResult {
    pub manifests: Vec<Manifest>,
    pub issues: Vec<LoadIssue>,
}

/// Reports the outcome of synchronizing one manifest file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyStatus {
    pub path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pipeline_name: String,
    pub applied: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
}

/// Reports one full manifest synchronization round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyReport {
    pub compatibility: String,
    pub manifest_directory: String,
    pub pipelines: Vec<ApplyStatus>,
}

/// Describes one repository pipeline declared through YAML.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub metadata: Metadata,
    #[serde(default)]
    pub spec: Spec,
    #[serde(skip)]
    pub path: PathBuf,
    #[serde(skip)]
    pub file_name: String,
}

/// Identifies one manifest uniquely inside the declarative directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub name: String,
}

/// Defines the pipeline sections declared by one manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    #[serde(default)]
    pub repository: RepositorySpec,
    #[serde(default)]
    pub credentials: Vec<CredentialSpec>,
    #[serde(default)]
    pub build: BuildSpec,
    #[serde(default)]
    pub publish: PublishSpec,
    #[serde(default)]
    pub bindings: Vec<BindingSpec>,
}

/// Defines the Git repository and polling behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RepositorySpec {
    #[serde(default)]
    pub engine: String,
    pub url: String,
    #[serde(rename = "defaultBranch", default)]
    pub default_branch: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(rename = "pollingIntervalSeconds", default)]
    pub polling_interval_seconds: i64,
    #[serde(default)]
    pub credentials: String,
}

/// Defines one named credential that other sections can reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CredentialSpec {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub basic: Option<BasicCredentialSpec>,
    #[serde(default)]
    pub bearer: Option<BearerCredentialSpec>,
    #[serde(default)]
    pub config: JsonMap<String, JsonValue>,
}

/// Defines one Git basic-auth credential payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct BasicCredentialSpec {
    pub username: ValueSource,
    pub password: ValueSource,
}

/// Defines one Git bearer-auth credential payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct BearerCredentialSpec {
    pub token: ValueSource,
}

/// Resolves one string from a literal, environment variable, or file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ValueSource {
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub env: String,
    #[serde(default)]
    pub file: String,
}

/// Defines all build targets declared for one repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct BuildSpec {
    #[serde(default)]
    pub targets: Vec<BuildTargetSpec>,
}

/// Defines one engine-aware build target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct BuildTargetSpec {
    pub name: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(rename = "buildKind", default)]
    pub build_kind: String,
    #[serde(default)]
    pub runner: RunnerSpec,
    #[serde(default)]
    pub output: OutputSpec,
    #[serde(default)]
    pub contract: BuildContractSpec,
    #[serde(default)]
    pub config: JsonMap<String, JsonValue>,
}

/// Defines the engine-scoped build contract for one target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct BuildContractSpec {
    #[serde(default)]
    pub unity: Option<UnityBuildContractSpec>,
}

/// Defines the Unity-specific build contract for one target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct UnityBuildContractSpec {
    #[serde(rename = "targetPlatform", default)]
    pub target_platform: String,
    #[serde(rename = "buildMethod", default)]
    pub build_method: String,
    #[serde(
        rename = "editorVersion",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub editor_version: String,
}

/// Describes runtime runner overrides for one target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RunnerSpec {
    #[serde(default, rename = "type")]
    pub runner_type: String,
    #[serde(rename = "timeoutSeconds", default)]
    pub timeout_seconds: i64,
}

/// Describes the artifact shape expected from one build method.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct OutputSpec {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub path: String,
}

/// Defines all publish targets declared for one repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PublishSpec {
    #[serde(default)]
    pub targets: Vec<PublishTargetSpec>,
}

/// Defines one publish destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PublishTargetSpec {
    pub name: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    pub kind: String,
    #[serde(default)]
    pub credentials: String,
    #[serde(default)]
    pub config: JsonMap<String, JsonValue>,
}

/// Declares one build-to-publish binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct BindingSpec {
    #[serde(rename = "buildTarget", default)]
    pub build_target: String,
    #[serde(rename = "publishTarget", default)]
    pub publish_target: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub options: JsonMap<String, JsonValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredCredential {
    id: i64,
    name: String,
    kind: String,
    config_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredRepository {
    id: i64,
    name: String,
    engine_kind: String,
    repo_url: String,
    credentials_id: Option<i64>,
    default_branch: Option<String>,
    polling_interval_seconds: i64,
    enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredBuildTarget {
    id: i64,
    repository_id: i64,
    name: String,
    build_kind: String,
    runner_type: String,
    output_kind: Option<String>,
    output_path_template: Option<String>,
    timeout_seconds: i64,
    enabled: bool,
    contract_json: String,
    config_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredPublishTarget {
    id: i64,
    repository_id: i64,
    name: String,
    kind: String,
    credentials_id: Option<i64>,
    enabled: bool,
    config_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredBinding {
    id: i64,
    build_target_id: i64,
    publish_target_id: i64,
    enabled: bool,
    options_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Synchronizer {
    database_path: PathBuf,
}

impl Manifest {
    /// Validates the minimum invariants required to synchronize one manifest.
    pub fn validate(&self) -> Result<(), String> {
        if self.api_version.trim() != API_VERSION {
            return Err(format!("apiVersion must be {API_VERSION:?}"));
        }
        if self.kind.trim() != MANIFEST_KIND {
            return Err(format!("kind must be {MANIFEST_KIND:?}"));
        }
        if self.metadata.name.trim().is_empty() {
            return Err(String::from("metadata.name must not be empty"));
        }
        if self.spec.repository.engine.trim().is_empty() {
            return Err(String::from("spec.repository.engine must not be empty"));
        }
        let repository_engine = self.spec.repository.engine.trim().to_ascii_lowercase();
        if repository_engine != SUPPORTED_ENGINE_UNITY {
            return Err(format!(
                "spec.repository.engine {:?} is not supported yet; only \"unity\" is accepted",
                self.spec.repository.engine.trim()
            ));
        }
        if self.spec.repository.url.trim().is_empty() {
            return Err(String::from("spec.repository.url must not be empty"));
        }
        if self.spec.repository.polling_interval_seconds < 0 {
            return Err(String::from(
                "spec.repository.pollingIntervalSeconds must not be negative",
            ));
        }

        let mut credentials_by_name = HashSet::with_capacity(self.spec.credentials.len());
        for credential in &self.spec.credentials {
            let name = credential.name.trim();
            if name.is_empty() {
                return Err(String::from("spec.credentials[].name must not be empty"));
            }
            if !credentials_by_name.insert(name.to_owned()) {
                return Err(format!("spec.credentials contains duplicate name {name:?}"));
            }

            let kind = credential.kind.trim().to_ascii_lowercase();
            if kind.is_empty() {
                return Err(format!("spec.credentials[{name:?}].kind must not be empty"));
            }

            match kind.as_str() {
                "git-http-basic" => {
                    let Some(basic) = &credential.basic else {
                        return Err(format!(
                            "spec.credentials[{name:?}].basic is required for \"git-http-basic\""
                        ));
                    };

                    basic.username.validate_required().map_err(|error| {
                        format!("spec.credentials[{name:?}].basic.username: {error}")
                    })?;
                    basic.password.validate_required().map_err(|error| {
                        format!("spec.credentials[{name:?}].basic.password: {error}")
                    })?;
                }
                "git-http-bearer" => {
                    let Some(bearer) = &credential.bearer else {
                        return Err(format!(
                            "spec.credentials[{name:?}].bearer is required for \"git-http-bearer\""
                        ));
                    };

                    bearer.token.validate_required().map_err(|error| {
                        format!("spec.credentials[{name:?}].bearer.token: {error}")
                    })?;
                }
                _ => {}
            }
        }

        if !self.spec.repository.credentials.trim().is_empty()
            && !credentials_by_name.contains(self.spec.repository.credentials.trim())
        {
            return Err(format!(
                "spec.repository.credentials references unknown credential {:?}",
                self.spec.repository.credentials.trim()
            ));
        }

        let mut build_targets = HashSet::with_capacity(self.spec.build.targets.len());
        for target in &self.spec.build.targets {
            let name = target.name.trim();
            if name.is_empty() {
                return Err(String::from("spec.build.targets[].name must not be empty"));
            }
            if !build_targets.insert(name.to_owned()) {
                return Err(format!("spec.build.targets contains duplicate name {name:?}"));
            }
            let build_kind = if target.build_kind.trim().is_empty() {
                DEFAULT_BUILD_KIND
            } else {
                target.build_kind.trim()
            };
            if !build_kind.eq_ignore_ascii_case(DEFAULT_BUILD_KIND) {
                return Err(format!(
                    "spec.build.targets[{name:?}].buildKind {:?} is not supported for engine \"unity\"; only \"player\" is accepted",
                    target.build_kind.trim()
                ));
            }

            let Some(unity_contract) = target.contract.unity.as_ref() else {
                return Err(format!(
                    "spec.build.targets[{name:?}].contract.unity is required when spec.repository.engine is \"unity\""
                ));
            };
            if unity_contract.target_platform.trim().is_empty() {
                return Err(format!(
                    "spec.build.targets[{name:?}].contract.unity.targetPlatform must not be empty"
                ));
            }
            if unity_contract.build_method.trim().is_empty() {
                return Err(format!(
                    "spec.build.targets[{name:?}].contract.unity.buildMethod must not be empty"
                ));
            }
            if target.output.kind.trim().is_empty() {
                return Err(format!("spec.build.targets[{name:?}].output.kind must not be empty"));
            }
            if target.output.path.trim().is_empty() {
                return Err(format!("spec.build.targets[{name:?}].output.path must not be empty"));
            }
            validate_requested_output_path(&target.output.kind, &target.output.path).map_err(
                |error| format!("spec.build.targets[{name:?}].output.path: {error}"),
            )?;
            if target.runner.timeout_seconds < 0 {
                return Err(format!(
                    "spec.build.targets[{name:?}].runner.timeoutSeconds must not be negative"
                ));
            }
        }

        let mut publish_targets = HashSet::with_capacity(self.spec.publish.targets.len());
        for target in &self.spec.publish.targets {
            let name = target.name.trim();
            if name.is_empty() {
                return Err(String::from("spec.publish.targets[].name must not be empty"));
            }
            if !publish_targets.insert(name.to_owned()) {
                return Err(format!("spec.publish.targets contains duplicate name {name:?}"));
            }
            if target.kind.trim().is_empty() {
                return Err(format!("spec.publish.targets[{name:?}].kind must not be empty"));
            }
            if !target.credentials.trim().is_empty()
                && !credentials_by_name.contains(target.credentials.trim())
            {
                return Err(format!(
                    "spec.publish.targets[{name:?}].credentials references unknown credential {:?}",
                    target.credentials.trim()
                ));
            }
        }

        let mut bindings = HashSet::with_capacity(self.spec.bindings.len());
        for binding in &self.spec.bindings {
            let build_target = binding.build_target.trim();
            if !build_targets.contains(build_target) {
                return Err(format!(
                    "spec.bindings references unknown build target {build_target:?}"
                ));
            }

            let publish_target = binding.publish_target.trim();
            if !publish_targets.contains(publish_target) {
                return Err(format!(
                    "spec.bindings references unknown publish target {publish_target:?}"
                ));
            }

            let key = binding_key(build_target, publish_target);
            if !bindings.insert(key.clone()) {
                return Err(format!("spec.bindings contains duplicate pair {key:?}"));
            }
        }

        Ok(())
    }
}

impl ValueSource {
    fn validate_required(&self) -> Result<(), String> {
        let mut count = 0;
        if !self.value.trim().is_empty() {
            count += 1;
        }
        if !self.env.trim().is_empty() {
            count += 1;
        }
        if !self.file.trim().is_empty() {
            count += 1;
        }

        if count != 1 {
            return Err(String::from("exactly one of value, env, or file must be set"));
        }

        Ok(())
    }

    fn resolve(&self) -> Result<String, String> {
        self.validate_required()?;

        if !self.value.trim().is_empty() {
            return Ok(self.value.trim().to_owned());
        }

        if !self.env.trim().is_empty() {
            let value = std::env::var(self.env.trim()).unwrap_or_default();
            let value = value.trim().to_owned();
            if value.is_empty() {
                return Err(format!(
                    "environment variable {:?} is empty",
                    self.env.trim()
                ));
            }
            return Ok(value);
        }

        let contents = fs::read_to_string(self.file.trim())
            .map_err(|error| format!("read file {:?}: {error}", self.file.trim()))?;
        let value = contents.trim().to_owned();
        if value.is_empty() {
            return Err(format!("file {:?} is empty", self.file.trim()));
        }

        Ok(value)
    }
}

impl Synchronizer {
    fn new(database_path: &Path) -> Self {
        Self {
            database_path: database_path.to_path_buf(),
        }
    }

    fn apply(&self, manifests: Vec<Manifest>, report: &mut ApplyReport) -> io::Result<()> {
        let mut connection = open_connection(&self.database_path)?;
        let mut credentials_by_name = list_credentials(&connection)?
            .into_iter()
            .map(|record| (record.name.clone(), record))
            .collect::<HashMap<_, _>>();
        let mut repositories_by_name = list_repositories(&connection)?
            .into_iter()
            .map(|record| (record.name.clone(), record))
            .collect::<HashMap<_, _>>();
        let mut active_repositories = HashSet::with_capacity(manifests.len());

        for manifest in manifests {
            let mut status = ApplyStatus {
                path: manifest.path.display().to_string(),
                pipeline_name: manifest.metadata.name.clone(),
                applied: false,
                error: String::new(),
            };

            let mut working_credentials = credentials_by_name.clone();
            let mut working_repositories = repositories_by_name.clone();
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(sqlite_error)?;

            match apply_manifest(
                &transaction,
                &manifest,
                &mut working_credentials,
                &mut working_repositories,
            ) {
                Ok(repository) => match transaction.commit() {
                    Ok(()) => {
                        credentials_by_name = working_credentials;
                        repositories_by_name = working_repositories;
                        active_repositories.insert(repository.name.clone());
                        status.applied = true;
                    }
                    Err(error) => {
                        status.error = sqlite_error_message(error);
                    }
                },
                Err(error) => {
                    status.error = error;
                }
            }

            report.pipelines.push(status);
        }

        disable_removed_repositories(
            &mut connection,
            &mut repositories_by_name,
            &active_repositories,
        )
    }
}

/// Loads every supported manifest file from one directory.
pub fn load_dir(dir: impl AsRef<Path>) -> io::Result<LoadResult> {
    let clean_dir = dir.as_ref();
    if clean_dir.as_os_str().is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "pipelines directory must not be empty",
        ));
    }

    let entries = match fs::read_dir(clean_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(LoadResult::default()),
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                format!(
                    "read pipelines directory {:?}: {error}",
                    clean_dir.display().to_string()
                ),
            ));
        }
    };

    let mut files = Vec::new();
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_file() {
            continue;
        }

        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.trim().is_empty() || name.starts_with('.') {
            continue;
        }
        if !is_manifest_extension(Path::new(name.as_ref()).extension()) {
            continue;
        }

        files.push(entry.path());
    }
    files.sort();

    let mut result = LoadResult {
        manifests: Vec::with_capacity(files.len()),
        issues: Vec::new(),
    };
    let mut seen_names = HashMap::with_capacity(files.len());

    for path in files {
        let manifest = match load_manifest(&path) {
            Ok(manifest) => manifest,
            Err(error) => {
                result.issues.push(LoadIssue {
                    path: path.display().to_string(),
                    error,
                });
                continue;
            }
        };

        if let Some(first_path) = seen_names.get(manifest.metadata.name.trim()) {
            result.issues.push(LoadIssue {
                path: manifest.path.display().to_string(),
                error: format!(
                    "duplicate metadata.name {:?} already declared by {}",
                    manifest.metadata.name,
                    first_path
                ),
            });
            continue;
        }

        seen_names.insert(
            manifest.metadata.name.trim().to_owned(),
            manifest.path.display().to_string(),
        );
        result.manifests.push(manifest);
    }

    Ok(result)
}

/// Loads one directory of manifests and synchronizes them into the runtime database.
pub fn sync_directory(
    database_path: impl AsRef<Path>,
    manifest_dir: impl AsRef<Path>,
) -> io::Result<ApplyReport> {
    let manifest_dir = manifest_dir.as_ref();
    let load_result = load_dir(manifest_dir)?;
    let mut report = ApplyReport {
        compatibility: ManifestCompatibility::FileSystem.description().to_owned(),
        manifest_directory: manifest_dir.display().to_string(),
        pipelines: Vec::with_capacity(load_result.manifests.len() + load_result.issues.len()),
    };

    for issue in load_result.issues {
        report.pipelines.push(ApplyStatus {
            path: issue.path,
            pipeline_name: String::new(),
            applied: false,
            error: issue.error,
        });
    }

    Synchronizer::new(database_path.as_ref()).apply(load_result.manifests, &mut report)?;
    Ok(report)
}

fn load_manifest(path: &Path) -> Result<Manifest, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("read manifest {:?}: {error}", path.display().to_string()))?;
    let mut manifest: Manifest = serde_yaml::from_str(&contents)
        .map_err(|error| format!("decode manifest {:?}: {error}", path.display().to_string()))?;
    manifest.path = path.to_path_buf();
    manifest.file_name = path
        .file_name()
        .unwrap_or_else(|| OsStr::new(""))
        .to_string_lossy()
        .to_string();
    manifest.validate().map_err(|error| {
        format!(
            "validate manifest {:?}: {error}",
            path.display().to_string()
        )
    })?;
    Ok(manifest)
}

fn apply_manifest(
    transaction: &Transaction<'_>,
    manifest: &Manifest,
    credentials_by_name: &mut HashMap<String, StoredCredential>,
    repositories_by_name: &mut HashMap<String, StoredRepository>,
) -> Result<StoredRepository, String> {
    let mut credential_ids = HashMap::with_capacity(manifest.spec.credentials.len());
    for credential_spec in &manifest.spec.credentials {
        let record = upsert_credential(transaction, manifest, credential_spec, credentials_by_name)?;
        credential_ids.insert(credential_spec.name.trim().to_owned(), record.id);
    }

    let repository = upsert_repository(
        transaction,
        manifest,
        repositories_by_name,
        &credential_ids,
    )?;
    let build_targets = sync_build_targets(transaction, manifest, &repository)?;
    let publish_targets = sync_publish_targets(transaction, manifest, &repository, &credential_ids)?;
    sync_bindings(transaction, manifest, &build_targets, &publish_targets)?;

    Ok(repository)
}

fn upsert_credential(
    transaction: &Transaction<'_>,
    manifest: &Manifest,
    credential_spec: &CredentialSpec,
    credentials_by_name: &mut HashMap<String, StoredCredential>,
) -> Result<StoredCredential, String> {
    let config_json = build_credential_config_json(credential_spec).map_err(|error| {
        format!(
            "sync credential {:?} in pipeline {:?}: {error}",
            credential_spec.name,
            manifest.metadata.name
        )
    })?;
    let record_name = credential_record_name(&manifest.metadata.name, &credential_spec.name);

    if let Some(existing) = credentials_by_name.get(&record_name).cloned() {
        let updated = update_credential(
            transaction,
            existing.id,
            &record_name,
            &credential_spec.kind,
            &config_json,
        )
        .map_err(|error| format!("update credential {record_name:?}: {error}"))?;
        credentials_by_name.insert(record_name, updated.clone());
        return Ok(updated);
    }

    let created = create_credential(transaction, &record_name, &credential_spec.kind, &config_json)
        .map_err(|error| format!("create credential {record_name:?}: {error}"))?;
    credentials_by_name.insert(record_name, created.clone());
    Ok(created)
}

fn upsert_repository(
    transaction: &Transaction<'_>,
    manifest: &Manifest,
    repositories_by_name: &mut HashMap<String, StoredRepository>,
    credential_ids: &HashMap<String, i64>,
) -> Result<StoredRepository, String> {
    let mut credentials_id = None;
    if !manifest.spec.repository.credentials.trim().is_empty() {
        let resolved = credential_ids
            .get(manifest.spec.repository.credentials.trim())
            .copied()
            .ok_or_else(|| {
                format!(
                    "pipeline {:?} references unknown repository credential {:?}",
                    manifest.metadata.name,
                    manifest.spec.repository.credentials.trim()
                )
            })?;
        credentials_id = Some(resolved);
    }

    let enabled = bool_value(manifest.spec.repository.enabled, true);
    let polling_interval = if manifest.spec.repository.polling_interval_seconds == 0 {
        300
    } else {
        manifest.spec.repository.polling_interval_seconds
    };
    let engine_kind = manifest.spec.repository.engine.trim().to_ascii_lowercase();

    if let Some(existing) = repositories_by_name.get(&manifest.metadata.name).cloned() {
        let updated = update_repository(
            transaction,
            existing.id,
            &manifest.metadata.name,
            &engine_kind,
            &manifest.spec.repository.url,
            credentials_id,
            &manifest.spec.repository.default_branch,
            polling_interval,
            enabled,
        )
        .map_err(|error| format!("update repository {:?}: {error}", manifest.metadata.name))?;
        repositories_by_name.insert(manifest.metadata.name.clone(), updated.clone());
        return Ok(updated);
    }

    let created = create_repository(
        transaction,
        &manifest.metadata.name,
        &engine_kind,
        &manifest.spec.repository.url,
        credentials_id,
        &manifest.spec.repository.default_branch,
        polling_interval,
        enabled,
    )
    .map_err(|error| format!("create repository {:?}: {error}", manifest.metadata.name))?;
    repositories_by_name.insert(manifest.metadata.name.clone(), created.clone());
    Ok(created)
}

fn sync_build_targets(
    transaction: &Transaction<'_>,
    manifest: &Manifest,
    repository: &StoredRepository,
) -> Result<HashMap<String, StoredBuildTarget>, String> {
    let repository_engine = manifest.spec.repository.engine.trim().to_ascii_lowercase();
    let existing_targets = list_build_targets(transaction, repository.id)
        .map_err(|error| format!("list build targets for repository {:?}: {error}", repository.name))?;
    let existing_by_name = existing_targets
        .iter()
        .cloned()
        .map(|target| (target.name.clone(), target))
        .collect::<HashMap<_, _>>();

    let mut active_names = HashSet::with_capacity(manifest.spec.build.targets.len());
    let mut resolved = HashMap::with_capacity(manifest.spec.build.targets.len());

    for target_spec in &manifest.spec.build.targets {
        let build_kind = if target_spec.build_kind.trim().is_empty() {
            DEFAULT_BUILD_KIND.to_owned()
        } else {
            target_spec.build_kind.trim().to_ascii_lowercase()
        };
        let contract_json = marshal_contract_json(&target_spec.contract).map_err(|error| {
            format!(
                "marshal build target {:?} contract: {error}",
                target_spec.name
            )
        })?;
        let config_json = marshal_json_object(&target_spec.config)
            .map_err(|error| format!("marshal build target {:?} config: {error}", target_spec.name))?;
        let enabled = bool_value(target_spec.enabled, true);
        validate_build_target_contract(
            repository_engine.as_str(),
            &build_kind,
            &contract_json,
        )?;

        let target = if let Some(existing) = existing_by_name.get(target_spec.name.trim()) {
            update_build_target(
                transaction,
                existing.id,
                &target_spec.name,
                &build_kind,
                runner_type(&target_spec.runner.runner_type),
                &target_spec.output.kind,
                &target_spec.output.path,
                runner_timeout(target_spec.runner.timeout_seconds),
                enabled,
                &contract_json,
                &config_json,
            )
            .map_err(|error| format!("update build target {:?}: {error}", target_spec.name))?
        } else {
            create_build_target(
                transaction,
                repository.id,
                &target_spec.name,
                &build_kind,
                runner_type(&target_spec.runner.runner_type),
                &target_spec.output.kind,
                &target_spec.output.path,
                runner_timeout(target_spec.runner.timeout_seconds),
                enabled,
                &contract_json,
                &config_json,
            )
            .map_err(|error| format!("create build target {:?}: {error}", target_spec.name))?
        };

        active_names.insert(target.name.clone());
        resolved.insert(target.name.clone(), target);
    }

    for existing in existing_targets {
        if active_names.contains(&existing.name) || !existing.enabled {
            continue;
        }

        let disabled = update_build_target(
            transaction,
            existing.id,
            &existing.name,
            &existing.build_kind,
            &existing.runner_type,
            existing.output_kind.as_deref().unwrap_or_default(),
            existing.output_path_template.as_deref().unwrap_or_default(),
            existing.timeout_seconds,
            false,
            &existing.contract_json,
            &existing.config_json,
        )
        .map_err(|error| format!("disable build target {:?}: {error}", existing.name))?;
        resolved.insert(disabled.name.clone(), disabled);
    }

    Ok(resolved)
}

fn sync_publish_targets(
    transaction: &Transaction<'_>,
    manifest: &Manifest,
    repository: &StoredRepository,
    credential_ids: &HashMap<String, i64>,
) -> Result<HashMap<String, StoredPublishTarget>, String> {
    let existing_targets = list_publish_targets(transaction, repository.id).map_err(|error| {
        format!(
            "list publish targets for repository {:?}: {error}",
            repository.name
        )
    })?;
    let existing_by_name = existing_targets
        .iter()
        .cloned()
        .map(|target| (target.name.clone(), target))
        .collect::<HashMap<_, _>>();

    let mut active_names = HashSet::with_capacity(manifest.spec.publish.targets.len());
    let mut resolved = HashMap::with_capacity(manifest.spec.publish.targets.len());

    for target_spec in &manifest.spec.publish.targets {
        let config_json = marshal_json_object(&target_spec.config).map_err(|error| {
            format!("marshal publish target {:?} config: {error}", target_spec.name)
        })?;

        let mut credentials_id = None;
        if !target_spec.credentials.trim().is_empty() {
            credentials_id = Some(*credential_ids.get(target_spec.credentials.trim()).ok_or_else(
                || {
                    format!(
                        "pipeline {:?} references unknown publish credential {:?} for target {:?}",
                        manifest.metadata.name,
                        target_spec.credentials.trim(),
                        target_spec.name
                    )
                },
            )?);
        }

        let enabled = bool_value(target_spec.enabled, true);
        let target = if let Some(existing) = existing_by_name.get(target_spec.name.trim()) {
            update_publish_target(
                transaction,
                existing.id,
                &target_spec.name,
                publish_kind(&target_spec.kind),
                credentials_id,
                enabled,
                &config_json,
            )
            .map_err(|error| format!("update publish target {:?}: {error}", target_spec.name))?
        } else {
            create_publish_target(
                transaction,
                repository.id,
                &target_spec.name,
                publish_kind(&target_spec.kind),
                credentials_id,
                enabled,
                &config_json,
            )
            .map_err(|error| format!("create publish target {:?}: {error}", target_spec.name))?
        };

        active_names.insert(target.name.clone());
        resolved.insert(target.name.clone(), target);
    }

    for existing in existing_targets {
        if active_names.contains(&existing.name) || !existing.enabled {
            continue;
        }

        let disabled = update_publish_target(
            transaction,
            existing.id,
            &existing.name,
            &existing.kind,
            existing.credentials_id,
            false,
            &existing.config_json,
        )
        .map_err(|error| format!("disable publish target {:?}: {error}", existing.name))?;
        resolved.insert(disabled.name.clone(), disabled);
    }

    Ok(resolved)
}

fn sync_bindings(
    transaction: &Transaction<'_>,
    manifest: &Manifest,
    build_targets: &HashMap<String, StoredBuildTarget>,
    publish_targets: &HashMap<String, StoredPublishTarget>,
) -> Result<(), String> {
    let mut active_keys = HashSet::with_capacity(manifest.spec.bindings.len());
    let mut existing_bindings = HashMap::new();

    for target in build_targets.values() {
        let bindings = list_bindings(transaction, target.id)
            .map_err(|error| format!("list bindings for build target {:?}: {error}", target.name))?;
        for binding in bindings {
            let publish_name = publish_target_name_by_id(publish_targets, binding.publish_target_id);
            if publish_name.is_empty() {
                continue;
            }

            existing_bindings.insert(binding_key(&target.name, &publish_name), binding);
        }
    }

    for binding_spec in &manifest.spec.bindings {
        let build_target = build_targets
            .get(binding_spec.build_target.trim())
            .ok_or_else(|| {
                format!(
                    "binding references unknown build target {:?}",
                    binding_spec.build_target
                )
            })?;
        let publish_target = publish_targets
            .get(binding_spec.publish_target.trim())
            .ok_or_else(|| {
                format!(
                    "binding references unknown publish target {:?}",
                    binding_spec.publish_target
                )
            })?;
        let options_json = marshal_json_object(&binding_spec.options).map_err(|error| {
            format!(
                "marshal binding {:?} -> {:?} options: {error}",
                binding_spec.build_target,
                binding_spec.publish_target
            )
        })?;
        let key = binding_key(&binding_spec.build_target, &binding_spec.publish_target);
        let enabled = bool_value(binding_spec.enabled, true);

        if let Some(existing) = existing_bindings.get(&key) {
            update_binding(transaction, existing.id, enabled, &options_json)
                .map_err(|error| format!("update binding {key}: {error}"))?;
        } else {
            create_binding(
                transaction,
                build_target.id,
                publish_target.id,
                enabled,
                &options_json,
            )
            .map_err(|error| format!("create binding {key}: {error}"))?;
        }

        active_keys.insert(key);
    }

    for (key, existing) in existing_bindings {
        if active_keys.contains(&key) || !existing.enabled {
            continue;
        }

        update_binding(transaction, existing.id, false, &existing.options_json)
            .map_err(|error| format!("disable binding {key}: {error}"))?;
    }

    Ok(())
}

fn disable_removed_repositories(
    connection: &mut Connection,
    repositories_by_name: &mut HashMap<String, StoredRepository>,
    active_repositories: &HashSet<String>,
) -> io::Result<()> {
    let mut repository_names = repositories_by_name.keys().cloned().collect::<Vec<_>>();
    repository_names.sort();

    for name in repository_names {
        if active_repositories.contains(&name) {
            continue;
        }

        let Some(existing) = repositories_by_name.get(&name).cloned() else {
            continue;
        };
        if !existing.enabled {
            continue;
        }

        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        let updated = update_repository(
            &transaction,
            existing.id,
            &existing.name,
            &existing.engine_kind,
            &existing.repo_url,
            existing.credentials_id,
            existing.default_branch.as_deref().unwrap_or_default(),
            existing.polling_interval_seconds,
            false,
        )
        .map_err(|error| io::Error::other(format!("disable repository {name:?}: {error}")))?;
        transaction.commit().map_err(sqlite_error)?;
        repositories_by_name.insert(name, updated);
    }

    Ok(())
}

fn open_connection(database_path: &Path) -> io::Result<Connection> {
    let connection = Connection::open(database_path).map_err(sqlite_error)?;
    connection.busy_timeout(std::time::Duration::from_millis(SQLITE_BUSY_TIMEOUT_MILLIS))
        .map_err(sqlite_error)?;
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(sqlite_error)?;
    Ok(connection)
}

fn list_credentials(connection: &Connection) -> io::Result<Vec<StoredCredential>> {
    let mut statement = connection
        .prepare(
            "
            SELECT id, name, kind, config_json
            FROM credentials
            ORDER BY id
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok(StoredCredential {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                config_json: row.get(3)?,
            })
        })
        .map_err(sqlite_error)?;

    collect_rows(rows)
}

fn list_repositories(connection: &Connection) -> io::Result<Vec<StoredRepository>> {
    let mut statement = connection
        .prepare(
            "
            SELECT id, name, engine_kind, repo_url, credentials_id, default_branch,
                   polling_interval_seconds, enabled
            FROM repositories
            ORDER BY id
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok(StoredRepository {
                id: row.get(0)?,
                name: row.get(1)?,
                engine_kind: row.get(2)?,
                repo_url: row.get(3)?,
                credentials_id: row.get(4)?,
                default_branch: row.get(5)?,
                polling_interval_seconds: row.get(6)?,
                enabled: row.get::<_, i64>(7)? != 0,
            })
        })
        .map_err(sqlite_error)?;

    collect_rows(rows)
}

fn list_build_targets(
    connection: &Connection,
    repository_id: i64,
) -> io::Result<Vec<StoredBuildTarget>> {
    let mut statement = connection
        .prepare(
            "
            SELECT id, repository_id, name, build_kind, runner_type,
                     output_kind, output_path_template, timeout_seconds,
                     enabled, contract_json, config_json
            FROM build_targets
            WHERE repository_id = ?
            ORDER BY id
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([repository_id], |row| {
            Ok(StoredBuildTarget {
                id: row.get(0)?,
                repository_id: row.get(1)?,
                name: row.get(2)?,
                build_kind: row.get(3)?,
                runner_type: row.get(4)?,
                output_kind: row.get(5)?,
                output_path_template: row.get(6)?,
                timeout_seconds: row.get(7)?,
                enabled: row.get::<_, i64>(8)? != 0,
                contract_json: row.get(9)?,
                config_json: row.get(10)?,
            })
        })
        .map_err(sqlite_error)?;

    collect_rows(rows)
}

fn list_publish_targets(
    connection: &Connection,
    repository_id: i64,
) -> io::Result<Vec<StoredPublishTarget>> {
    let mut statement = connection
        .prepare(
            "
            SELECT id, repository_id, name, kind, credentials_id, enabled, config_json
            FROM publish_targets
            WHERE repository_id = ?
            ORDER BY id
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([repository_id], |row| {
            Ok(StoredPublishTarget {
                id: row.get(0)?,
                repository_id: row.get(1)?,
                name: row.get(2)?,
                kind: row.get(3)?,
                credentials_id: row.get(4)?,
                enabled: row.get::<_, i64>(5)? != 0,
                config_json: row.get(6)?,
            })
        })
        .map_err(sqlite_error)?;

    collect_rows(rows)
}

fn list_bindings(connection: &Connection, build_target_id: i64) -> io::Result<Vec<StoredBinding>> {
    let mut statement = connection
        .prepare(
            "
            SELECT id, build_target_id, publish_target_id, enabled, options_json
            FROM build_publish_bindings
            WHERE build_target_id = ?
            ORDER BY id
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([build_target_id], |row| {
            Ok(StoredBinding {
                id: row.get(0)?,
                build_target_id: row.get(1)?,
                publish_target_id: row.get(2)?,
                enabled: row.get::<_, i64>(3)? != 0,
                options_json: row.get(4)?,
            })
        })
        .map_err(sqlite_error)?;

    collect_rows(rows)
}

fn create_credential(
    transaction: &Transaction<'_>,
    name: &str,
    kind: &str,
    config_json: &str,
) -> rusqlite::Result<StoredCredential> {
    transaction.execute(
        "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
        params![name.trim(), kind.trim(), config_json],
    )?;
    get_credential(transaction, transaction.last_insert_rowid())
}

fn update_credential(
    transaction: &Transaction<'_>,
    id: i64,
    name: &str,
    kind: &str,
    config_json: &str,
) -> rusqlite::Result<StoredCredential> {
    transaction.execute(
        "
        UPDATE credentials
        SET name = ?, kind = ?, config_json = ?, updated_at = CURRENT_TIMESTAMP
        WHERE id = ?
        ",
        params![name.trim(), kind.trim(), config_json, id],
    )?;
    get_credential(transaction, id)
}

fn get_credential(connection: &Connection, id: i64) -> rusqlite::Result<StoredCredential> {
    connection.query_row(
        "SELECT id, name, kind, config_json FROM credentials WHERE id = ?",
        [id],
        |row| {
            Ok(StoredCredential {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                config_json: row.get(3)?,
            })
        },
    )
}

fn create_repository(
    transaction: &Transaction<'_>,
    name: &str,
    engine_kind: &str,
    repo_url: &str,
    credentials_id: Option<i64>,
    default_branch: &str,
    polling_interval_seconds: i64,
    enabled: bool,
) -> rusqlite::Result<StoredRepository> {
    transaction.execute(
        "
        INSERT INTO repositories (
            name,
            engine_kind,
            repo_url,
            credentials_id,
            default_branch,
            polling_interval_seconds,
            enabled
        )
        VALUES (?, ?, ?, ?, ?, ?, ?)
        ",
        params![
            name.trim(),
            engine_kind.trim(),
            repo_url.trim(),
            credentials_id,
            nullable_string(default_branch),
            polling_interval_seconds,
            bool_to_int(enabled),
        ],
    )?;
    get_repository(transaction, transaction.last_insert_rowid())
}

fn update_repository(
    transaction: &Transaction<'_>,
    id: i64,
    name: &str,
    engine_kind: &str,
    repo_url: &str,
    credentials_id: Option<i64>,
    default_branch: &str,
    polling_interval_seconds: i64,
    enabled: bool,
) -> rusqlite::Result<StoredRepository> {
    transaction.execute(
        "
        UPDATE repositories
        SET name = ?,
            engine_kind = ?,
            repo_url = ?,
            credentials_id = ?,
            default_branch = ?,
            polling_interval_seconds = ?,
            enabled = ?,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?
        ",
        params![
            name.trim(),
            engine_kind.trim(),
            repo_url.trim(),
            credentials_id,
            nullable_string(default_branch),
            polling_interval_seconds,
            bool_to_int(enabled),
            id,
        ],
    )?;
    get_repository(transaction, id)
}

fn get_repository(connection: &Connection, id: i64) -> rusqlite::Result<StoredRepository> {
    connection.query_row(
        "
        SELECT id, name, engine_kind, repo_url, credentials_id, default_branch,
               polling_interval_seconds, enabled
        FROM repositories
        WHERE id = ?
        ",
        [id],
        |row| {
            Ok(StoredRepository {
                id: row.get(0)?,
                name: row.get(1)?,
                engine_kind: row.get(2)?,
                repo_url: row.get(3)?,
                credentials_id: row.get(4)?,
                default_branch: row.get(5)?,
                polling_interval_seconds: row.get(6)?,
                enabled: row.get::<_, i64>(7)? != 0,
            })
        },
    )
}

fn create_build_target(
    transaction: &Transaction<'_>,
    repository_id: i64,
    name: &str,
    build_kind: &str,
    runner_type: &str,
    output_kind: &str,
    output_path_template: &str,
    timeout_seconds: i64,
    enabled: bool,
    contract_json: &str,
    config_json: &str,
) -> rusqlite::Result<StoredBuildTarget> {
    transaction.execute(
        "
        INSERT INTO build_targets (
            repository_id,
            name,
            build_kind,
            runner_type,
            output_kind,
            output_path_template,
            timeout_seconds,
            enabled,
            contract_json,
            config_json
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ",
        params![
            repository_id,
            name.trim(),
            build_kind.trim(),
            runner_type.trim(),
            nullable_string(output_kind),
            nullable_string(output_path_template),
            timeout_seconds,
            bool_to_int(enabled),
            contract_json,
            config_json,
        ],
    )?;
    get_build_target(transaction, transaction.last_insert_rowid())
}

#[allow(clippy::too_many_arguments)]
fn update_build_target(
    transaction: &Transaction<'_>,
    id: i64,
    name: &str,
    build_kind: &str,
    runner_type: &str,
    output_kind: &str,
    output_path_template: &str,
    timeout_seconds: i64,
    enabled: bool,
    contract_json: &str,
    config_json: &str,
) -> rusqlite::Result<StoredBuildTarget> {
    transaction.execute(
        "
        UPDATE build_targets
        SET name = ?,
            build_kind = ?,
            runner_type = ?,
            output_kind = ?,
            output_path_template = ?,
            timeout_seconds = ?,
            enabled = ?,
            contract_json = ?,
            config_json = ?,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?
        ",
        params![
            name.trim(),
            build_kind.trim(),
            runner_type.trim(),
            nullable_string(output_kind),
            nullable_string(output_path_template),
            timeout_seconds,
            bool_to_int(enabled),
            contract_json,
            config_json,
            id,
        ],
    )?;
    get_build_target(transaction, id)
}

fn get_build_target(connection: &Connection, id: i64) -> rusqlite::Result<StoredBuildTarget> {
    connection.query_row(
        "
        SELECT id, repository_id, name, build_kind, runner_type,
             output_kind, output_path_template, timeout_seconds,
             enabled, contract_json, config_json
        FROM build_targets
        WHERE id = ?
        ",
        [id],
        |row| {
            Ok(StoredBuildTarget {
                id: row.get(0)?,
                repository_id: row.get(1)?,
                name: row.get(2)?,
                build_kind: row.get(3)?,
                runner_type: row.get(4)?,
                output_kind: row.get(5)?,
                output_path_template: row.get(6)?,
                timeout_seconds: row.get(7)?,
                enabled: row.get::<_, i64>(8)? != 0,
                contract_json: row.get(9)?,
                config_json: row.get(10)?,
            })
        },
    )
}

fn create_publish_target(
    transaction: &Transaction<'_>,
    repository_id: i64,
    name: &str,
    kind: &str,
    credentials_id: Option<i64>,
    enabled: bool,
    config_json: &str,
) -> rusqlite::Result<StoredPublishTarget> {
    transaction.execute(
        "
        INSERT INTO publish_targets (
            repository_id,
            name,
            kind,
            credentials_id,
            enabled,
            config_json
        )
        VALUES (?, ?, ?, ?, ?, ?)
        ",
        params![
            repository_id,
            name.trim(),
            kind.trim(),
            credentials_id,
            bool_to_int(enabled),
            config_json,
        ],
    )?;
    get_publish_target(transaction, transaction.last_insert_rowid())
}

fn update_publish_target(
    transaction: &Transaction<'_>,
    id: i64,
    name: &str,
    kind: &str,
    credentials_id: Option<i64>,
    enabled: bool,
    config_json: &str,
) -> rusqlite::Result<StoredPublishTarget> {
    transaction.execute(
        "
        UPDATE publish_targets
        SET name = ?,
            kind = ?,
            credentials_id = ?,
            enabled = ?,
            config_json = ?,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?
        ",
        params![
            name.trim(),
            kind.trim(),
            credentials_id,
            bool_to_int(enabled),
            config_json,
            id,
        ],
    )?;
    get_publish_target(transaction, id)
}

fn get_publish_target(connection: &Connection, id: i64) -> rusqlite::Result<StoredPublishTarget> {
    connection.query_row(
        "
        SELECT id, repository_id, name, kind, credentials_id, enabled, config_json
        FROM publish_targets
        WHERE id = ?
        ",
        [id],
        |row| {
            Ok(StoredPublishTarget {
                id: row.get(0)?,
                repository_id: row.get(1)?,
                name: row.get(2)?,
                kind: row.get(3)?,
                credentials_id: row.get(4)?,
                enabled: row.get::<_, i64>(5)? != 0,
                config_json: row.get(6)?,
            })
        },
    )
}

fn create_binding(
    transaction: &Transaction<'_>,
    build_target_id: i64,
    publish_target_id: i64,
    enabled: bool,
    options_json: &str,
) -> rusqlite::Result<StoredBinding> {
    transaction.execute(
        "
        INSERT INTO build_publish_bindings (
            build_target_id,
            publish_target_id,
            enabled,
            options_json
        )
        VALUES (?, ?, ?, ?)
        ",
        params![
            build_target_id,
            publish_target_id,
            bool_to_int(enabled),
            options_json,
        ],
    )?;
    get_binding(transaction, transaction.last_insert_rowid())
}

fn update_binding(
    transaction: &Transaction<'_>,
    id: i64,
    enabled: bool,
    options_json: &str,
) -> rusqlite::Result<StoredBinding> {
    transaction.execute(
        "
        UPDATE build_publish_bindings
        SET enabled = ?,
            options_json = ?,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = ?
        ",
        params![bool_to_int(enabled), options_json, id],
    )?;
    get_binding(transaction, id)
}

fn get_binding(connection: &Connection, id: i64) -> rusqlite::Result<StoredBinding> {
    connection.query_row(
        "
        SELECT id, build_target_id, publish_target_id, enabled, options_json
        FROM build_publish_bindings
        WHERE id = ?
        ",
        [id],
        |row| {
            Ok(StoredBinding {
                id: row.get(0)?,
                build_target_id: row.get(1)?,
                publish_target_id: row.get(2)?,
                enabled: row.get::<_, i64>(3)? != 0,
                options_json: row.get(4)?,
            })
        },
    )
}

fn build_credential_config_json(spec: &CredentialSpec) -> Result<String, String> {
    match spec.kind.trim().to_ascii_lowercase().as_str() {
        "git-http-basic" => {
            let basic = spec
                .basic
                .as_ref()
                .ok_or_else(|| String::from("basic credential payload is required"))?;
            let username = basic.username.resolve()?;
            let password = basic.password.resolve()?;
            let mut object = JsonMap::new();
            object.insert(String::from("username"), JsonValue::String(username));
            object.insert(String::from("password"), JsonValue::String(password));
            marshal_json_object(&object)
        }
        "git-http-bearer" => {
            let bearer = spec
                .bearer
                .as_ref()
                .ok_or_else(|| String::from("bearer credential payload is required"))?;
            let token = bearer.token.resolve()?;
            let mut object = JsonMap::new();
            object.insert(String::from("token"), JsonValue::String(token));
            marshal_json_object(&object)
        }
        _ => marshal_json_object(&spec.config),
    }
}

fn marshal_json_object(value: &JsonMap<String, JsonValue>) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| error.to_string())
}

fn marshal_contract_json(value: &BuildContractSpec) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| error.to_string())
}

fn validate_build_target_contract(
    repository_engine: &str,
    build_kind: &str,
    contract_json: &str,
) -> Result<(), String> {
    let normalized_build_kind = if build_kind.trim().is_empty() {
        DEFAULT_BUILD_KIND.to_owned()
    } else {
        build_kind.trim().to_ascii_lowercase()
    };

    match repository_engine {
        SUPPORTED_ENGINE_UNITY => {
            if normalized_build_kind != DEFAULT_BUILD_KIND {
                return Err(format!(
                    "build target contract projection does not support buildKind {:?} for engine \"unity\"",
                    normalized_build_kind
                ));
            }

            let trimmed_contract_json = contract_json.trim();
            if trimmed_contract_json.is_empty() {
                return Err(String::from(
                    "build target contract_json must not be empty for engine \"unity\"",
                ));
            }

            let contract = serde_json::from_str::<BuildContractSpec>(trimmed_contract_json)
                .map_err(|error| {
                    format!(
                        "build target contract_json must match the supported engine contract schema: {error}"
                    )
                })?;
            let Some(unity) = contract.unity else {
                return Err(String::from(
                    "build target contract_json must define contract.unity for engine \"unity\"",
                ));
            };

            let platform = unity.target_platform.trim();
            let Some(_) = normalize_optional_owned_string(&unity.build_method) else {
                return Err(String::from(
                    "build target contract_json must define contract.unity.buildMethod for engine \"unity\"",
                ));
            };
            if platform.is_empty() {
                return Err(String::from(
                    "build target contract_json must define contract.unity.targetPlatform for engine \"unity\"",
                ));
            }

            Ok(())
        }
        other => Err(format!(
            "build target contract projection does not support engine {other:?}"
        )),
    }
}

fn validate_requested_output_path(output_kind: &str, output_path_template: &str) -> Result<(), String> {
    let trimmed_kind = output_kind.trim();
    let trimmed_path = output_path_template.trim();
    if trimmed_kind.is_empty() || trimmed_path.is_empty() {
        return Ok(());
    }

    let normalized_path = trimmed_path.replace('\\', "/").to_ascii_lowercase();
    if trimmed_kind.eq_ignore_ascii_case("archive") && normalized_path.ends_with(".zip") {
        return Err(String::from(
            "archive output_path_template is a requested build path, not the final artifact filename; remove the .zip suffix",
        ));
    }

    Ok(())
}

fn is_manifest_extension(extension: Option<&OsStr>) -> bool {
    matches!(
        extension.and_then(OsStr::to_str).map(|value| value.to_ascii_lowercase()),
        Some(value) if value == "yml" || value == "yaml"
    )
}

fn credential_record_name(pipeline_name: &str, credential_name: &str) -> String {
    format!("{}/{}", pipeline_name.trim(), credential_name.trim())
}

fn binding_key(build_target_name: &str, publish_target_name: &str) -> String {
    format!("{}->{}", build_target_name.trim(), publish_target_name.trim())
}

fn publish_target_name_by_id(
    targets: &HashMap<String, StoredPublishTarget>,
    publish_target_id: i64,
) -> String {
    targets
        .iter()
        .find_map(|(name, target)| {
            if target.id == publish_target_id {
                Some(name.clone())
            } else {
                None
            }
        })
        .unwrap_or_default()
}

fn runner_type(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        DEFAULT_RUNNER_TYPE
    } else {
        trimmed
    }
}

fn runner_timeout(value: i64) -> i64 {
    if value <= 0 {
        DEFAULT_TIMEOUT_SECONDS
    } else {
        value
    }
}

fn publish_kind(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        DEFAULT_PUBLISH_KIND
    } else {
        trimmed
    }
}

fn bool_value(value: Option<bool>, fallback: bool) -> bool {
    value.unwrap_or(fallback)
}

fn normalize_optional_owned_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn nullable_string(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn bool_to_int(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn collect_rows<T>(rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>) -> io::Result<Vec<T>> {
    let mut collected = Vec::new();
    for row in rows {
        collected.push(row.map_err(sqlite_error)?);
    }
    Ok(collected)
}

fn sqlite_error(error: rusqlite::Error) -> io::Error {
    io::Error::other(sqlite_error_message(error))
}

fn sqlite_error_message(error: rusqlite::Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::{load_dir, sync_directory};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::{params, Connection};
    use runtime_config::RuntimeConfig;
    use runtime_store::{initialize_database, StorageLayout};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn load_dir_and_sync_directory_synchronizes_pipeline_manifest() {
        std::env::set_var("REVOLUTIONS_GIT_USERNAME", "git");
        std::env::set_var("REVOLUTIONS_GIT_TOKEN", "secret-token");

        let root = test_root("sync-directory");
        let pipelines_dir = root.join("pipelines");
        fs::create_dir_all(&pipelines_dir).expect("pipelines directory should exist");
        write_manifest(
            &pipelines_dir.join("revolutions.yml"),
            concat!(
                "apiVersion: handy.games.publisher/v1alpha1\n",
                "kind: Pipeline\n",
                "metadata:\n",
                "  name: revolutions\n",
                "spec:\n",
                "  repository:\n",
                "    engine: unity\n",
                "    url: https://example.com/org/revolutions.git\n",
                "    defaultBranch: main\n",
                "    pollingIntervalSeconds: 300\n",
                "    credentials: origin\n",
                "  credentials:\n",
                "    - name: origin\n",
                "      kind: git-http-basic\n",
                "      basic:\n",
                "        username:\n",
                "          env: REVOLUTIONS_GIT_USERNAME\n",
                "        password:\n",
                "          env: REVOLUTIONS_GIT_TOKEN\n",
                "  build:\n",
                "    targets:\n",
                "      - name: linux64\n",
                "        buildKind: player\n",
                "        runner:\n",
                "          timeoutSeconds: 5400\n",
                "        output:\n",
                "          kind: archive\n",
                "          path: Builds/Linux64\n",
                "        contract:\n",
                "          unity:\n",
                "            targetPlatform: StandaloneLinux64\n",
                "            buildMethod: Builder.BuildLinux64\n",
                "            editorVersion: 2022.3.14f1\n",
                "        config:\n",
                "          compression: zip\n",
                "  publish:\n",
                "    targets:\n",
                "      - name: filesystem-release\n",
                "        kind: filesystem\n",
                "        config: {}\n",
                "  bindings:\n",
                "    - buildTarget: linux64\n",
                "      publishTarget: filesystem-release\n",
                "      options:\n",
                "        operation: move\n",
                "        directory_path: /exports/releases\n"
            ),
        );

        let storage = initialize_test_database(&root);
        let report = sync_directory(&storage.database_path, &pipelines_dir)
            .expect("manifest sync should succeed");
        assert_eq!(report.pipelines.len(), 1);
        assert!(report.pipelines[0].applied);

        let connection = Connection::open(&storage.database_path)
            .expect("sqlite database should open for verification");

        let credential = connection
            .query_row(
                "SELECT name, kind, config_json FROM credentials",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .expect("credential record should exist");
        assert_eq!(credential.0, "revolutions/origin");
        assert_eq!(credential.1, "git-http-basic");
        assert_eq!(credential.2, r#"{"password":"secret-token","username":"git"}"#);

        let repository_id = connection
            .query_row(
                "SELECT id FROM repositories WHERE name = ?",
                ["revolutions"],
                |row| row.get::<_, i64>(0),
            )
            .expect("repository record should exist");
        let repository = connection
            .query_row(
                "SELECT engine_kind, credentials_id, default_branch, enabled FROM repositories WHERE id = ?",
                [repository_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .expect("repository fields should load");
        assert_eq!(repository.0, "unity");
        assert!(repository.1.is_some());
        assert_eq!(repository.2.as_deref(), Some("main"));
        assert_eq!(repository.3, 1);

        let build_target_id = connection
            .query_row(
                "SELECT id FROM build_targets WHERE repository_id = ? AND name = ?",
                params![repository_id, "linux64"],
                |row| row.get::<_, i64>(0),
            )
            .expect("build target should exist");
        let build_target = connection
            .query_row(
                "SELECT build_kind, contract_json, runner_type, timeout_seconds, config_json FROM build_targets WHERE id = ?",
                [build_target_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .expect("build target fields should load");
        assert_eq!(build_target.0, "player");
        assert_eq!(
            build_target.1,
            r#"{"unity":{"targetPlatform":"StandaloneLinux64","buildMethod":"Builder.BuildLinux64","editorVersion":"2022.3.14f1"}}"#
        );
        assert_eq!(build_target.2, "host-native");
        assert_eq!(build_target.3, 5400);
        assert_eq!(build_target.4, r#"{"compression":"zip"}"#);

        let publish_target_id = connection
            .query_row(
                "SELECT id FROM publish_targets WHERE repository_id = ? AND name = ?",
                params![repository_id, "filesystem-release"],
                |row| row.get::<_, i64>(0),
            )
            .expect("publish target should exist");
        let publish_target = connection
            .query_row(
                "SELECT kind, config_json FROM publish_targets WHERE id = ?",
                [publish_target_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .expect("publish target fields should load");
        assert_eq!(publish_target.0, "filesystem");
        assert_eq!(publish_target.1, "{}");

        let binding = connection
            .query_row(
                "SELECT options_json, enabled FROM build_publish_bindings WHERE build_target_id = ? AND publish_target_id = ?",
                params![build_target_id, publish_target_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .expect("binding should exist");
        assert_eq!(
            binding.0,
            r#"{"directory_path":"/exports/releases","operation":"move"}"#
        );
        assert_eq!(binding.1, 1);
    }

    #[test]
    fn sync_directory_disables_removed_repository_and_target() {
        let root = test_root("disable-removed");
        let pipelines_dir = root.join("pipelines");
        fs::create_dir_all(&pipelines_dir).expect("pipelines directory should exist");
        let manifest_path = pipelines_dir.join("alpha.yml");
        write_manifest(
            &manifest_path,
            concat!(
                "apiVersion: handy.games.publisher/v1alpha1\n",
                "kind: Pipeline\n",
                "metadata:\n",
                "  name: alpha\n",
                "spec:\n",
                "  repository:\n",
                "    engine: unity\n",
                "    url: https://example.com/org/alpha.git\n",
                "    pollingIntervalSeconds: 300\n",
                "  build:\n",
                "    targets:\n",
                "      - name: linux64\n",
                "        buildKind: player\n",
                "        output:\n",
                "          kind: archive\n",
                "          path: Builds/Linux64\n",
                "        contract:\n",
                "          unity:\n",
                "            targetPlatform: StandaloneLinux64\n",
                "            buildMethod: Builder.BuildLinux64\n",
                "  publish:\n",
                "    targets: []\n",
                "  bindings: []\n"
            ),
        );

        let storage = initialize_test_database(&root);
        sync_directory(&storage.database_path, &pipelines_dir)
            .expect("first manifest sync should succeed");

        write_manifest(
            &manifest_path,
            concat!(
                "apiVersion: handy.games.publisher/v1alpha1\n",
                "kind: Pipeline\n",
                "metadata:\n",
                "  name: alpha\n",
                "spec:\n",
                "  repository:\n",
                "    engine: unity\n",
                "    url: https://example.com/org/alpha.git\n",
                "    enabled: false\n",
                "    pollingIntervalSeconds: 300\n",
                "  build:\n",
                "    targets: []\n",
                "  publish:\n",
                "    targets: []\n",
                "  bindings: []\n"
            ),
        );

        sync_directory(&storage.database_path, &pipelines_dir)
            .expect("second manifest sync should succeed");

        let connection = Connection::open(&storage.database_path)
            .expect("sqlite database should open for verification");
        let repository = connection
            .query_row(
                "SELECT enabled FROM repositories WHERE name = ?",
                ["alpha"],
                |row| row.get::<_, i64>(0),
            )
            .expect("repository should exist");
        assert_eq!(repository, 0);

        let build_target = connection
            .query_row(
                "SELECT enabled, contract_json FROM build_targets WHERE name = ?",
                ["linux64"],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                    ))
                },
            )
            .expect("build target should exist");
        assert_eq!(build_target.0, 0);
        assert_eq!(
            build_target.1,
            r#"{"unity":{"targetPlatform":"StandaloneLinux64","buildMethod":"Builder.BuildLinux64"}}"#
        );
    }

    #[test]
    fn load_dir_reports_actionable_issue_for_archive_zip_suffix() {
        let root = test_root("zip-guidance");
        let pipelines_dir = root.join("pipelines");
        fs::create_dir_all(&pipelines_dir).expect("pipelines directory should exist");
        write_manifest(
            &pipelines_dir.join("revolutions.yml"),
            concat!(
                "apiVersion: handy.games.publisher/v1alpha1\n",
                "kind: Pipeline\n",
                "metadata:\n",
                "  name: revolutions\n",
                "spec:\n",
                "  repository:\n",
                "    engine: unity\n",
                "    url: https://example.com/org/revolutions.git\n",
                "  build:\n",
                "    targets:\n",
                "      - name: webgl\n",
                "        buildKind: player\n",
                "        output:\n",
                "          kind: archive\n",
                "          path: Builds/WebGL.zip\n",
                "        contract:\n",
                "          unity:\n",
                "            targetPlatform: WebGL\n",
                "            buildMethod: Builder.PerformWebGL\n",
                "  publish:\n",
                "    targets: []\n",
                "  bindings: []\n"
            ),
        );

        let load_result = load_dir(&pipelines_dir).expect("manifest load should succeed");
        assert!(load_result.manifests.is_empty());
        assert_eq!(load_result.issues.len(), 1);
        assert!(load_result.issues[0].error.contains("remove the .zip suffix"));
    }

    #[test]
    fn sync_directory_resolves_file_backed_bearer_credentials() {
        let root = test_root("file-credential");
        let pipelines_dir = root.join("pipelines");
        fs::create_dir_all(&pipelines_dir).expect("pipelines directory should exist");
        let token_path = root.join("token.txt");
        fs::write(&token_path, " revolution-token\n")
            .expect("token file should be written");
        write_manifest(
            &pipelines_dir.join("alpha.yml"),
            format!(
                concat!(
                    "apiVersion: handy.games.publisher/v1alpha1\n",
                    "kind: Pipeline\n",
                    "metadata:\n",
                    "  name: alpha\n",
                    "spec:\n",
                    "  repository:\n",
                    "    engine: unity\n",
                    "    url: https://example.com/org/alpha.git\n",
                    "  credentials:\n",
                    "    - name: publish-token\n",
                    "      kind: git-http-bearer\n",
                    "      bearer:\n",
                    "        token:\n",
                    "          file: {}\n",
                    "  build:\n",
                    "    targets: []\n",
                    "  publish:\n",
                    "    targets:\n",
                    "      - name: releases\n",
                    "        kind: filesystem\n",
                    "        credentials: publish-token\n",
                    "        config: {{}}\n",
                    "  bindings: []\n"
                ),
                token_path.display()
            ),
        );

        let storage = initialize_test_database(&root);
        sync_directory(&storage.database_path, &pipelines_dir)
            .expect("manifest sync should succeed");

        let connection = Connection::open(&storage.database_path)
            .expect("sqlite database should open for verification");
        let credential = connection
            .query_row(
                "SELECT config_json FROM credentials WHERE name = ?",
                ["alpha/publish-token"],
                |row| row.get::<_, String>(0),
            )
            .expect("bearer credential should exist");
        assert_eq!(credential, r#"{"token":"revolution-token"}"#);
    }

    #[test]
    fn load_dir_rejects_non_unity_repository_engine() {
        let root = test_root("unsupported-engine");
        let pipelines_dir = root.join("pipelines");
        fs::create_dir_all(&pipelines_dir).expect("pipelines directory should exist");
        write_manifest(
            &pipelines_dir.join("alpha.yml"),
            concat!(
                "apiVersion: handy.games.publisher/v1alpha1\n",
                "kind: Pipeline\n",
                "metadata:\n",
                "  name: alpha\n",
                "spec:\n",
                "  repository:\n",
                "    engine: unreal\n",
                "    url: https://example.com/org/alpha.git\n",
                "  build:\n",
                "    targets: []\n",
                "  publish:\n",
                "    targets: []\n",
                "  bindings: []\n"
            ),
        );

        let load_result = load_dir(&pipelines_dir).expect("manifest load should succeed");
        assert!(load_result.manifests.is_empty());
        assert_eq!(load_result.issues.len(), 1);
        assert!(load_result.issues[0].error.contains("only \"unity\" is accepted"));
    }

    #[test]
    fn load_dir_rejects_non_player_unity_build_kind() {
        let root = test_root("unsupported-build-kind");
        let pipelines_dir = root.join("pipelines");
        fs::create_dir_all(&pipelines_dir).expect("pipelines directory should exist");
        write_manifest(
            &pipelines_dir.join("alpha.yml"),
            concat!(
                "apiVersion: handy.games.publisher/v1alpha1\n",
                "kind: Pipeline\n",
                "metadata:\n",
                "  name: alpha\n",
                "spec:\n",
                "  repository:\n",
                "    engine: unity\n",
                "    url: https://example.com/org/alpha.git\n",
                "  build:\n",
                "    targets:\n",
                "      - name: dedicated-server\n",
                "        buildKind: server\n",
                "        output:\n",
                "          kind: directory\n",
                "          path: Builds/Server\n",
                "        contract:\n",
                "          unity:\n",
                "            targetPlatform: StandaloneLinux64\n",
                "            buildMethod: Builder.PerformServer\n",
                "  publish:\n",
                "    targets: []\n",
                "  bindings: []\n"
            ),
        );

        let load_result = load_dir(&pipelines_dir).expect("manifest load should succeed");
        assert!(load_result.manifests.is_empty());
        assert_eq!(load_result.issues.len(), 1);
        assert!(load_result.issues[0].error.contains("only \"player\" is accepted"));
    }

    fn initialize_test_database(root: &Path) -> StorageLayout {
        let config = RuntimeConfig::from_root(root.join("runtime"));
        config
            .directories
            .ensure_exists()
            .expect("runtime directories should exist");
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");
        storage
    }

    fn write_manifest(path: &Path, contents: impl AsRef<str>) {
        fs::write(path, contents.as_ref()).expect("manifest file should be written");
    }

    fn test_root(label: &str) -> PathBuf {
        let token = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "runtime-manifests-{label}-{now}-{token}"
        ));
        fs::create_dir_all(&root).expect("test root should be created");
        root
    }
}