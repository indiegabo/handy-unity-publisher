# Process Reset Playbook

Use this playbook to clear all persisted process state from the current HUP
runtime so the app behaves like a first execution for releases, builds,
publishes, polling baselines, retained process artifacts, and runtime event
history.

## Target Database

Resolve the database path using the same rules as `runtime-config`:

1. Use `HANDY_UNITY_PUBLISHER_RUNTIME_ROOT` when it is set.
2. On Windows, otherwise use `%LOCALAPPDATA%\handy-unity-publisher\runtime`.
3. Append `state/runtime.db`.

Stop the runtime or shell automation loop first when practical so it does not
recreate process state during the reset.

## What To Delete

- All rows from `execution_cleanup_records`
- All rows from `retained_execution_files`
- All rows from `build_run_steps`
- All rows from `publish_runs`
- All rows from `artifacts`
- All rows from `build_runs`
- All rows from `release_runs`
- All `worker_queue_messages` in `release-runs`, `build-runs`, and
  `publish-runs`
- All `worker_coordination_leases` whose names match `release-run:*`,
  `release-plan:*`, `build-run:*`, or `publish-run:*`
- All `worker_idempotency_keys` whose keys match `release-run:*`,
  `build-run:*`, or `publish-run:*`
- Every repository `last_seen_tag`
- Every runtime-owned process workspace under `runs/`
- Every runtime-owned retained artifact entry under `artifacts/`
- Runtime process history files:
  `state/runtime-events.jsonl`, `state/runtime-events.cursor.json`,
  `state/health.json`, `state/supervisor-state.json`, and `logs/runtime.jsonl`

Do not change:

- Repository definitions
- Build targets
- Publish targets
- Credentials
- Runtime database file and migrations

## One-Shot Script Template

Prefer running this through `execution_subagent` so the result comes back with
the command summary.

```text
py -3 - <<'PY'
import json
import os
import pathlib
import shutil
import sqlite3

PROCESS_TABLES = [
    'execution_cleanup_records',
    'retained_execution_files',
    'build_run_steps',
    'publish_runs',
    'artifacts',
    'build_runs',
    'release_runs',
]
PROCESS_QUEUE_NAMES = ('release-runs', 'build-runs', 'publish-runs')
LEASE_PATTERNS = (
    'release-run:*',
    'release-plan:*',
    'build-run:*',
    'publish-run:*',
)
IDEMPOTENCY_PATTERNS = (
    'release-run:*',
    'build-run:*',
    'publish-run:*',
)

runtime_root = os.environ.get('HANDY_UNITY_PUBLISHER_RUNTIME_ROOT')
if runtime_root:
    runtime_root = pathlib.Path(runtime_root)
else:
    local_app_data = os.environ.get('LOCALAPPDATA') or os.environ.get('APPDATA')
    if not local_app_data:
        raise SystemExit('missing LOCALAPPDATA or APPDATA')
    runtime_root = pathlib.Path(local_app_data) / 'handy-unity-publisher' / 'runtime'

state_dir = runtime_root / 'state'
logs_dir = runtime_root / 'logs'
artifacts_dir = runtime_root / 'artifacts'
runs_dir = runtime_root / 'runs'
db_path = state_dir / 'runtime.db'

if not db_path.is_file():
    raise SystemExit(f'database not found: {db_path}')

tracked_state_files = [
    state_dir / 'runtime-events.jsonl',
    state_dir / 'runtime-events.cursor.json',
    state_dir / 'health.json',
    state_dir / 'supervisor-state.json',
    logs_dir / 'runtime.jsonl',
]

def fetch_scalar(connection, sql, params=()):
    return connection.execute(sql, params).fetchone()[0]

def table_exists(connection, table_name):
    return fetch_scalar(
        connection,
        "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = ?",
        (table_name,),
    ) > 0

def count_if_exists(connection, table_name):
    if not table_exists(connection, table_name):
        return None
    return fetch_scalar(connection, f'SELECT COUNT(1) FROM {table_name}')

def count_pattern_rows(connection, table_name, column_name, patterns):
    where_clause = ' OR '.join(f"{column_name} GLOB ?" for _ in patterns)
    return fetch_scalar(
        connection,
        f"SELECT COUNT(1) FROM {table_name} WHERE {where_clause}",
        patterns,
    )

def clear_directory_contents(path):
    removed = []
    if path.exists() and path.is_dir():
        for child in sorted(path.iterdir(), key=lambda candidate: candidate.name):
            removed.append(child.name)
            if child.is_dir():
                shutil.rmtree(child)
            else:
                child.unlink()
    path.mkdir(parents=True, exist_ok=True)
    return removed

def list_dir_children(path):
    if not path.exists() or not path.is_dir():
        return []
    return sorted(child.name for child in path.iterdir())

def remove_file(path):
    existed = path.exists()
    if existed:
        path.unlink()
    return existed

with sqlite3.connect(db_path) as connection:
    connection.execute('PRAGMA foreign_keys = ON')
    counts_before = {table_name: count_if_exists(connection, table_name) for table_name in PROCESS_TABLES}
    counts_before['process_queue_messages'] = fetch_scalar(
        connection,
        'SELECT COUNT(1) FROM worker_queue_messages WHERE queue_name IN (?, ?, ?)',
        PROCESS_QUEUE_NAMES,
    )
    counts_before['process_coordination_leases'] = count_pattern_rows(
        connection,
        'worker_coordination_leases',
        'name',
        LEASE_PATTERNS,
    )
    counts_before['process_idempotency_keys'] = count_pattern_rows(
        connection,
        'worker_idempotency_keys',
        'idempotency_key',
        IDEMPOTENCY_PATTERNS,
    )
    counts_before['repositories_with_last_seen_tag'] = fetch_scalar(
        connection,
        "SELECT COUNT(1) FROM repositories WHERE last_seen_tag IS NOT NULL AND TRIM(last_seen_tag) <> ''",
    )

    connection.execute('BEGIN IMMEDIATE TRANSACTION')
    deleted_counts = {}
    deleted_counts['process_queue_messages'] = connection.execute(
        'DELETE FROM worker_queue_messages WHERE queue_name IN (?, ?, ?)',
        PROCESS_QUEUE_NAMES,
    ).rowcount
    deleted_counts['process_coordination_leases'] = connection.execute(
        'DELETE FROM worker_coordination_leases WHERE name GLOB ? OR name GLOB ? OR name GLOB ? OR name GLOB ?',
        LEASE_PATTERNS,
    ).rowcount
    deleted_counts['process_idempotency_keys'] = connection.execute(
        'DELETE FROM worker_idempotency_keys WHERE idempotency_key GLOB ? OR idempotency_key GLOB ? OR idempotency_key GLOB ?',
        IDEMPOTENCY_PATTERNS,
    ).rowcount
    for table_name in PROCESS_TABLES:
        deleted_counts[table_name] = connection.execute(
            f'DELETE FROM {table_name}'
        ).rowcount if table_exists(connection, table_name) else None
    deleted_counts['repositories_last_seen_tag_reset'] = connection.execute(
        "UPDATE repositories SET last_seen_tag = NULL WHERE last_seen_tag IS NOT NULL AND TRIM(last_seen_tag) <> ''"
    ).rowcount
    connection.commit()

removed_run_entries = clear_directory_contents(runs_dir)
removed_artifact_entries = clear_directory_contents(artifacts_dir)
removed_state_files = [
    str(path) for path in tracked_state_files if remove_file(path)
]

with sqlite3.connect(db_path) as verification_connection:
    verification_connection.execute('PRAGMA foreign_keys = ON')
    counts_after = {table_name: count_if_exists(verification_connection, table_name) for table_name in PROCESS_TABLES}
    counts_after['process_queue_messages'] = fetch_scalar(
        verification_connection,
        'SELECT COUNT(1) FROM worker_queue_messages WHERE queue_name IN (?, ?, ?)',
        PROCESS_QUEUE_NAMES,
    )
    counts_after['process_coordination_leases'] = count_pattern_rows(
        verification_connection,
        'worker_coordination_leases',
        'name',
        LEASE_PATTERNS,
    )
    counts_after['process_idempotency_keys'] = count_pattern_rows(
        verification_connection,
        'worker_idempotency_keys',
        'idempotency_key',
        IDEMPOTENCY_PATTERNS,
    )
    counts_after['repositories_with_last_seen_tag'] = fetch_scalar(
        verification_connection,
        "SELECT COUNT(1) FROM repositories WHERE last_seen_tag IS NOT NULL AND TRIM(last_seen_tag) <> ''",
    )

report = {
    'runtime_root': str(runtime_root),
    'database_path': str(db_path),
    'counts_before': counts_before,
    'deleted_counts': deleted_counts,
    'counts_after': counts_after,
    'filesystem': {
        'runs_dir': str(runs_dir),
        'runs_entries_removed': removed_run_entries,
        'runs_entries_after': list_dir_children(runs_dir),
        'artifacts_dir': str(artifacts_dir),
        'artifacts_entries_removed': removed_artifact_entries,
        'artifacts_entries_after': list_dir_children(artifacts_dir),
        'state_files_removed': removed_state_files,
    },
}
print(json.dumps(report, ensure_ascii=True))
PY
```

## Expected Result

Return a compact JSON object containing:

- `runtime_root`
- `database_path`
- `counts_before`
- `deleted_counts`
- `counts_after`
- `filesystem`

If the runtime is already clean, the command should still succeed and report
zero counts after verification.

## Known Pitfall

The Rust example at `crates/runtime-store/examples/reset_repository_processes.rs`
is repository-scoped and does not clear runtime-owned process workspaces,
retained artifacts, or runtime event files. Do not use it when the user asks
to leave the app as a first execution with respect to process state.
`last_seen_tag`.
