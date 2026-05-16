---
name: hup-reset-runtime-processes
description: "Reset all HGP process state so the app behaves like a first execution for releases, builds, publishes, polling baselines, runtime event history, and process workspaces while preserving repository and pipeline definitions."
argument-hint: "[no arguments]"
user-invocable: true
---

# HGP Reset Runtime Processes

Use this skill when you need to remove all persisted process state from the
current local runtime so the app behaves like a fresh first run for anything
related to releases, builds, publishes, polling baselines, retained process
history, and runtime execution traces.

## When To Use

- Reset all process history across every repository in the active runtime
- Clear release/build/publish rows, related child tables, queue messages,
  coordination leases, idempotency keys, and polling baselines
- Remove process workspaces and retained artifacts from the runtime-owned
  filesystem
- Clear runtime event and execution log files that would otherwise keep stale
  process history visible
- Handle requests such as `zerar processos`, `limpar processos`,
  `clear runtime runs`, or `wipe current execution state`
- Leave repository registrations, build targets, publish targets,
  credentials, and pipeline definitions intact

## Critical Rules

1. Path resolution: resolve the database path from
   `HANDY_GAMES_PUBLISHER_RUNTIME_ROOT`; on Windows without that runtime root,
   use `%LOCALAPPDATA%\HandyGamesPublisher\runtime\state\runtime.db`. If
   neither rule resolves a database path, stop and report that the runtime
   root could not be resolved.
2. Runtime quiescence: stop the runtime or shell automation loop first if it
   is actively creating, updating, or cleaning process state.
3. Polling baseline: treat `repositories.last_seen_tag` as process state and
   reset it to `NULL` for every repository.
4. Process row deletion order: delete process rows in child-first order:
   `execution_cleanup_records`, `retained_execution_files`,
   `build_run_steps`, `publish_runs`, `artifacts`, `build_runs`, and
   `release_runs`.
5. Queue cleanup: delete all process queue messages in `release-runs`,
   `build-runs`, and `publish-runs`.
6. Lease cleanup: delete all process coordination leases matching
   `release-run:*`, `release-plan:*`, `build-run:*`, and `publish-run:*`.
7. Idempotency cleanup: delete all process idempotency keys matching
   `release-run:*`, `build-run:*`, and `publish-run:*`.
8. Filesystem cleanup: clear process-owned runtime directories such as
   `runs/` and `artifacts/` after the database transaction succeeds, then
   remove runtime process history files such as `runtime-events.jsonl`,
   `runtime-events.cursor.json`, `health.json`, `supervisor-state.json`, and
   `logs/runtime.jsonl`.
9. Scope guard: do not use
   `crates/runtime-store/examples/reset_repository_processes.rs` for this job;
   it is repository-scoped and does not clear filesystem process traces.
10. Verification: reopen the database and verify the counts again after the
    transaction.

## Procedure

1. Resolve the live runtime root and database path using the runtime-config
   rules for the host.
2. Prefer one `execution_subagent` call that runs a one-shot standard-library
   Python script.
3. Capture before-counts for all process tables, all process queue messages,
   process coordination leases, process idempotency keys, and repositories with
   populated `last_seen_tag`.
4. In one immediate transaction, delete all process rows in child-first order,
   then delete process queue messages, process coordination leases, and process
   idempotency keys.
5. In the same transaction, set `repositories.last_seen_tag = NULL` for every
   repository so polling restarts from a first-run baseline.
6. After the transaction commits, empty the runtime-owned `runs/` and
   `artifacts/` directories while keeping those directories themselves.
7. Remove runtime process-history files from `state/` and `logs/`.
8. Reopen the database, recount the same slices, and report before/after
   counts in compact JSON.

## Validation

- Verify `release_runs`, `build_runs`, `publish_runs`, `artifacts`,
  `build_run_steps`, `execution_cleanup_records`, and
  `retained_execution_files` are all zero after reopening the database
- Verify process queue messages, process coordination leases, and process
  idempotency keys are all zero after reopening the database
- Verify the number of repositories with `last_seen_tag` is zero
- Verify `runs/` and `artifacts/` exist and are empty after cleanup
- Mention which state/log files were removed in the final report

## References

- [Process reset playbook](./references/repository-reset-playbook.md)
