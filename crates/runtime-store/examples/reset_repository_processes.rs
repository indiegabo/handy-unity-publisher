use runtime_config::RuntimeConfig;
use runtime_store::{
    open_connection, BuildDispatchJob, PublishDispatchJob, ReleaseDispatchJob, StorageLayout,
};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior};
use serde::Serialize;
use std::collections::HashSet;
use std::io::{self, ErrorKind};

const BUILD_RUN_QUEUE_NAME: &str = "build-runs";
const PUBLISH_RUN_QUEUE_NAME: &str = "publish-runs";
const RELEASE_RUN_QUEUE_NAME: &str = "release-runs";

fn main() -> io::Result<()> {
    let repository_name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| String::from("Revolutions"));

    let config = RuntimeConfig::load()?;
    let storage = StorageLayout::from_directories(&config.directories);
    let mut connection = open_connection(&storage.database_path)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(io::Error::other)?;

    let repository = load_repository(&transaction, &repository_name)?;
    let release_run_ids = load_release_run_ids(&transaction, repository.id)?;
    let build_run_ids = load_build_run_ids(&transaction, repository.id)?;
    let publish_run_ids = load_publish_run_ids(&transaction, repository.id)?;
    let queue_message_ids = load_target_queue_message_ids(
        &transaction,
        repository.id,
        &release_run_ids,
        &build_run_ids,
        &publish_run_ids,
    )?;
    let coordination_lease_names = load_target_coordination_lease_names(
        &transaction,
        &release_run_ids,
        &build_run_ids,
        &publish_run_ids,
    )?;
    let idempotency_keys = load_target_idempotency_keys(
        &transaction,
        &release_run_ids,
        &build_run_ids,
        &publish_run_ids,
    )?;

    delete_queue_messages(&transaction, &queue_message_ids)?;
    delete_coordination_leases(&transaction, &coordination_lease_names)?;
    delete_idempotency_keys(&transaction, &idempotency_keys)?;

    let cleared_release_runs = transaction
        .execute(
            "DELETE FROM release_runs WHERE repository_id = ?",
            [repository.id],
        )
        .map_err(io::Error::other)? as u64;
    transaction
        .execute(
            "UPDATE repositories SET last_seen_tag = NULL WHERE id = ?",
            [repository.id],
        )
        .map_err(io::Error::other)?;

    transaction.commit().map_err(io::Error::other)?;

    let report = ResetRepositoryProcessesReport {
        repository_id: repository.id,
        repository_name: repository.name,
        previous_last_seen_tag: repository.last_seen_tag,
        cleared_release_runs,
        cleared_build_runs: build_run_ids.len() as u64,
        cleared_publish_runs: publish_run_ids.len() as u64,
        cleared_queue_messages: queue_message_ids.len() as u64,
        cleared_coordination_leases: coordination_lease_names.len() as u64,
        cleared_idempotency_keys: idempotency_keys.len() as u64,
        last_seen_tag_after: None,
    };

    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(io::Error::other)?
    );

    Ok(())
}

#[derive(Debug, Serialize)]
struct ResetRepositoryProcessesReport {
    repository_id: i64,
    repository_name: String,
    previous_last_seen_tag: Option<String>,
    cleared_release_runs: u64,
    cleared_build_runs: u64,
    cleared_publish_runs: u64,
    cleared_queue_messages: u64,
    cleared_coordination_leases: u64,
    cleared_idempotency_keys: u64,
    last_seen_tag_after: Option<String>,
}

struct RepositoryRow {
    id: i64,
    name: String,
    last_seen_tag: Option<String>,
}

fn load_repository(
    transaction: &Transaction<'_>,
    repository_name: &str,
) -> io::Result<RepositoryRow> {
    transaction
        .query_row(
            "
            SELECT id, name, last_seen_tag
            FROM repositories
            WHERE name = ?
            ",
            [repository_name],
            |row| {
                Ok(RepositoryRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    last_seen_tag: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(io::Error::other)?
        .ok_or_else(|| {
            io::Error::new(
                ErrorKind::NotFound,
                format!("repository {:?} was not found", repository_name),
            )
        })
}

fn load_release_run_ids(
    transaction: &Transaction<'_>,
    repository_id: i64,
) -> io::Result<HashSet<i64>> {
    load_identifier_set(
        transaction,
        "
        SELECT id
        FROM release_runs
        WHERE repository_id = ?
        ",
        repository_id,
    )
}

fn load_build_run_ids(
    transaction: &Transaction<'_>,
    repository_id: i64,
) -> io::Result<HashSet<i64>> {
    load_identifier_set(
        transaction,
        "
        SELECT br.id
        FROM build_runs br
        JOIN release_runs rr ON rr.id = br.release_run_id
        WHERE rr.repository_id = ?
        ",
        repository_id,
    )
}

fn load_publish_run_ids(
    transaction: &Transaction<'_>,
    repository_id: i64,
) -> io::Result<HashSet<i64>> {
    load_identifier_set(
        transaction,
        "
        SELECT pr.id
        FROM publish_runs pr
        JOIN release_runs rr ON rr.id = pr.release_run_id
        WHERE rr.repository_id = ?
        ",
        repository_id,
    )
}

fn load_identifier_set(
    transaction: &Transaction<'_>,
    sql: &str,
    repository_id: i64,
) -> io::Result<HashSet<i64>> {
    let mut statement = transaction.prepare(sql).map_err(io::Error::other)?;
    let rows = statement
        .query_map([repository_id], |row| row.get::<_, i64>(0))
        .map_err(io::Error::other)?;

    let mut identifiers = HashSet::new();
    for row in rows {
        identifiers.insert(row.map_err(io::Error::other)?);
    }

    Ok(identifiers)
}

fn load_target_queue_message_ids(
    transaction: &Transaction<'_>,
    repository_id: i64,
    release_run_ids: &HashSet<i64>,
    build_run_ids: &HashSet<i64>,
    publish_run_ids: &HashSet<i64>,
) -> io::Result<Vec<i64>> {
    let mut statement = transaction
        .prepare(
            "
            SELECT id, queue_name, payload
            FROM worker_queue_messages
            ORDER BY id ASC
            ",
        )
        .map_err(io::Error::other)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })
        .map_err(io::Error::other)?;

    let mut queue_message_ids = Vec::new();
    for row in rows {
        let (message_id, queue_name, payload) = row.map_err(io::Error::other)?;
        let belongs_to_repository = match queue_name.as_str() {
            RELEASE_RUN_QUEUE_NAME => serde_json::from_slice::<ReleaseDispatchJob>(&payload)
                .map(|job| job.repository_id == repository_id)
                .map_err(io::Error::other)?,
            BUILD_RUN_QUEUE_NAME => serde_json::from_slice::<BuildDispatchJob>(&payload)
                .map(|job| {
                    release_run_ids.contains(&job.release_run_id)
                        || build_run_ids.contains(&job.build_run_id)
                })
                .map_err(io::Error::other)?,
            PUBLISH_RUN_QUEUE_NAME => serde_json::from_slice::<PublishDispatchJob>(&payload)
                .map(|job| {
                    release_run_ids.contains(&job.release_run_id)
                        || build_run_ids.contains(&job.build_run_id)
                        || publish_run_ids.contains(&job.publish_run_id)
                })
                .map_err(io::Error::other)?,
            _ => false,
        };

        if belongs_to_repository {
            queue_message_ids.push(message_id);
        }
    }

    Ok(queue_message_ids)
}

fn load_target_coordination_lease_names(
    transaction: &Transaction<'_>,
    release_run_ids: &HashSet<i64>,
    build_run_ids: &HashSet<i64>,
    publish_run_ids: &HashSet<i64>,
) -> io::Result<Vec<String>> {
    let mut statement = transaction
        .prepare(
            "
            SELECT name
            FROM worker_coordination_leases
            ORDER BY name ASC
            ",
        )
        .map_err(io::Error::other)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(io::Error::other)?;

    let mut names = Vec::new();
    for row in rows {
        let name = row.map_err(io::Error::other)?;
        if matches_release_lock_name(&name, release_run_ids)
            || matches_build_lock_name(&name, build_run_ids)
            || matches_publish_lock_name(&name, publish_run_ids)
        {
            names.push(name);
        }
    }

    Ok(names)
}

fn load_target_idempotency_keys(
    transaction: &Transaction<'_>,
    release_run_ids: &HashSet<i64>,
    build_run_ids: &HashSet<i64>,
    publish_run_ids: &HashSet<i64>,
) -> io::Result<Vec<String>> {
    let mut statement = transaction
        .prepare(
            "
            SELECT idempotency_key
            FROM worker_idempotency_keys
            ORDER BY idempotency_key ASC
            ",
        )
        .map_err(io::Error::other)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(io::Error::other)?;

    let mut keys = Vec::new();
    for row in rows {
        let key = row.map_err(io::Error::other)?;
        if matches_release_idempotency_key(&key, release_run_ids)
            || matches_build_idempotency_key(&key, build_run_ids)
            || matches_publish_idempotency_key(&key, publish_run_ids)
        {
            keys.push(key);
        }
    }

    Ok(keys)
}

fn matches_release_lock_name(name: &str, release_run_ids: &HashSet<i64>) -> bool {
    release_run_ids.iter().any(|release_run_id| {
        name == format!("release-run:{release_run_id}:dispatch")
            || name == format!("release-plan:{release_run_id}")
    })
}

fn matches_build_lock_name(name: &str, build_run_ids: &HashSet<i64>) -> bool {
    build_run_ids
        .iter()
        .any(|build_run_id| name == format!("build-run:{build_run_id}:dispatch"))
}

fn matches_publish_lock_name(name: &str, publish_run_ids: &HashSet<i64>) -> bool {
    publish_run_ids
        .iter()
        .any(|publish_run_id| name == format!("publish-run:{publish_run_id}:dispatch"))
}

fn matches_release_idempotency_key(key: &str, release_run_ids: &HashSet<i64>) -> bool {
    release_run_ids
        .iter()
        .any(|release_run_id| key == format!("release-run:{release_run_id}:queued"))
}

fn matches_build_idempotency_key(key: &str, build_run_ids: &HashSet<i64>) -> bool {
    build_run_ids.iter().any(|build_run_id| {
        key == format!("build-run:{build_run_id}:queued")
            || key.starts_with(&format!("build-run:{build_run_id}:"))
    })
}

fn matches_publish_idempotency_key(key: &str, publish_run_ids: &HashSet<i64>) -> bool {
    publish_run_ids.iter().any(|publish_run_id| {
        key == format!("publish-run:{publish_run_id}:queued")
            || key.starts_with(&format!("publish-run:{publish_run_id}:"))
    })
}

fn delete_queue_messages(
    transaction: &Transaction<'_>,
    queue_message_ids: &[i64],
) -> io::Result<()> {
    let mut statement = transaction
        .prepare("DELETE FROM worker_queue_messages WHERE id = ?")
        .map_err(io::Error::other)?;
    for queue_message_id in queue_message_ids {
        statement
            .execute([queue_message_id])
            .map_err(io::Error::other)?;
    }

    Ok(())
}

fn delete_coordination_leases(
    transaction: &Transaction<'_>,
    coordination_lease_names: &[String],
) -> io::Result<()> {
    let mut statement = transaction
        .prepare("DELETE FROM worker_coordination_leases WHERE name = ?")
        .map_err(io::Error::other)?;
    for coordination_lease_name in coordination_lease_names {
        statement
            .execute([coordination_lease_name])
            .map_err(io::Error::other)?;
    }

    Ok(())
}

fn delete_idempotency_keys(
    transaction: &Transaction<'_>,
    idempotency_keys: &[String],
) -> io::Result<()> {
    let mut statement = transaction
        .prepare("DELETE FROM worker_idempotency_keys WHERE idempotency_key = ?")
        .map_err(io::Error::other)?;
    for idempotency_key in idempotency_keys {
        statement
            .execute([idempotency_key])
            .map_err(io::Error::other)?;
    }

    Ok(())
}
