use runtime_config::RuntimeConfig;
use runtime_store::{
    open_connection, store_host_secret, LocalCoordinator, StorageLayout,
    UpsertCredentialRecordInput, HOST_KEYRING_SERVICE, KEYRING_SECRET_REF_PREFIX,
};
use rusqlite::OptionalExtension;
use serde_json::Value;
use std::io::{self, ErrorKind, Write};
use std::time::{SystemTime, UNIX_EPOCH};

fn main() -> io::Result<()> {
    let mut arguments = std::env::args().skip(1);
    let repository_name = arguments
        .next()
        .unwrap_or_else(|| String::from("Revolutions"));
    let inline_secret = arguments.any(|argument| argument == "--inline");

    eprint!("Enter PAT for {repository_name}: ");
    io::stderr().flush()?;

    let mut personal_access_token = String::new();
    io::stdin().read_line(&mut personal_access_token)?;
    let personal_access_token = personal_access_token.trim().to_owned();
    if personal_access_token.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "personal access token must not be empty",
        ));
    }

    let config = RuntimeConfig::load()?;
    let storage = StorageLayout::from_directories(&config.directories);
    let binding = load_repository_binding(&storage, &repository_name)?;
    let coordinator = LocalCoordinator::new(&storage);
    let credentials = coordinator.get_credential_record(binding.credentials_id)?;
    let config_json = rebind_credential_config_json(
        &binding.repository_url,
        &credentials.kind,
        &credentials.config_json,
        &personal_access_token,
        inline_secret,
    )?;

    coordinator.upsert_credential_record(UpsertCredentialRecordInput {
        credential_id: Some(credentials.id),
        name: credentials.name,
        kind: credentials.kind,
        config_json,
    })?;

    println!(
        "rebound repository {} credential {}",
        binding.repository_id, binding.credentials_id
    );

    Ok(())
}

struct RepositoryCredentialBinding {
    repository_id: i64,
    credentials_id: i64,
    repository_url: String,
}

fn load_repository_binding(
    storage: &StorageLayout,
    repository_name: &str,
) -> io::Result<RepositoryCredentialBinding> {
    let connection = open_connection(&storage.database_path)?;
    let row = connection
        .query_row(
            "
            SELECT r.id, r.credentials_id, r.repo_url
            FROM repositories r
            WHERE r.name = ?
            ",
            [repository_name],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(io::Error::other)?;

    let Some((repository_id, credentials_id, repository_url)) = row else {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!("repository {:?} was not found", repository_name),
        ));
    };
    let Some(credentials_id) = credentials_id else {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!(
                "repository {:?} does not have bound credentials",
                repository_name
            ),
        ));
    };

    Ok(RepositoryCredentialBinding {
        repository_id,
        credentials_id,
        repository_url,
    })
}

fn rebind_credential_config_json(
    repository_url: &str,
    kind: &str,
    config_json: &str,
    personal_access_token: &str,
    inline_secret: bool,
) -> io::Result<String> {
    let secret_key = match kind.trim() {
        "git-http-basic" => "password",
        "git-http-bearer" => "token",
        other => {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!("unsupported credentials kind {other:?}"),
            ))
        }
    };

    let mut parsed = serde_json::from_str::<Value>(config_json)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
    let object = parsed.as_object_mut().ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidData,
            "credentials config_json must decode to a JSON object",
        )
    })?;

    let secret_value = if inline_secret {
        personal_access_token.to_owned()
    } else {
        let existing_secret_ref = object
            .get(secret_key)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty());
        let account = existing_secret_ref
            .map(keyring_account_from_secret_ref)
            .transpose()?
            .unwrap_or_else(|| repair_secret_account(repository_url));

        store_host_secret(&account, personal_access_token)?
    };

    object.insert(String::from(secret_key), Value::String(secret_value));
    if kind.trim() == "git-http-basic" {
        let username = object
            .get("username")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if username.is_empty() || username.eq_ignore_ascii_case("git") {
            if let Some(owner) = github_repository_owner(repository_url) {
                object.insert(String::from("username"), Value::String(owner));
            }
        }
    }

    serde_json::to_string(&parsed).map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
}

fn keyring_account_from_secret_ref(secret_ref: &str) -> io::Result<String> {
    let reference_tail = secret_ref
        .strip_prefix(KEYRING_SECRET_REF_PREFIX)
        .ok_or_else(|| {
            io::Error::new(
                ErrorKind::InvalidInput,
                "secret reference must begin with keyring://",
            )
        })?;
    let (service, account) = reference_tail.split_once('/').ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "secret reference must follow keyring://<service>/<account>",
        )
    })?;
    if !service.eq_ignore_ascii_case(HOST_KEYRING_SERVICE) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("unsupported keyring service {service:?}"),
        ));
    }

    Ok(account.to_owned())
}

fn repair_secret_account(repository_url: &str) -> String {
    let issued_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!(
        "repository/{}/origin-pat-rebind/{issued_at}",
        slugify(repository_url)
    )
}

fn github_repository_owner(repository_url: &str) -> Option<String> {
    let repository_url = repository_url.trim();
    let without_scheme = repository_url
        .strip_prefix("https://")
        .or_else(|| repository_url.strip_prefix("http://"))?;
    let (host, path) = without_scheme.split_once('/')?;
    if !host.eq_ignore_ascii_case("github.com") {
        return None;
    }

    let owner = path.split('/').next()?.trim();
    if owner.is_empty() {
        return None;
    }

    Some(owner.to_owned())
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_was_separator = false;

    for character in value.chars() {
        let lowered = character.to_ascii_lowercase();
        if lowered.is_ascii_alphanumeric() {
            slug.push(lowered);
            previous_was_separator = false;
            continue;
        }

        if !previous_was_separator && !slug.is_empty() {
            slug.push('-');
            previous_was_separator = true;
        }
    }

    if slug.is_empty() {
        String::from("repository")
    } else {
        slug.trim_matches('-').to_owned()
    }
}
