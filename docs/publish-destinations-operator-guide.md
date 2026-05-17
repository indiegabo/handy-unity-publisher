# Publish Destinations Operator Guide

This guide documents the operator-facing contract for publish destinations in
the HGP desktop application.

## Desktop authoring flow

Repository projects can define zero or more publish destinations.

- Zero-destination projects remain valid. Their artifacts stay in the
  runtime-managed output root after build completion.
- The create-project wizard exposes a dedicated `Publish Destinations` step
  after `Build Targets`.
- Repository detail exposes a `Publish Destinations` section for editing the
  same destination and binding data after project creation.

Each destination owns a destination-wide configuration and one or more
per-build-target bindings.

## Binding semantics

Bindings are evaluated per artifact.

- `itch` bindings are non-consuming. They publish the artifact without
  rewriting the artifact's active location.
- `filesystem` bindings are consuming. They move the artifact into the bound
  absolute directory and rewrite the artifact's active location on success.
- HGP allows at most one enabled consuming binding per build target.
- When both binding kinds exist for the same build target, all non-consuming
  bindings execute before the single consuming binding.

Removing a build target that still owns bindings requires confirmation in the
desktop UI. Removing a destination that still owns bindings also requires
confirmation. Changing a destination kind requires confirmation when the
existing configuration would be invalidated.

## Filesystem move behavior

Filesystem destinations use binding-specific absolute directories.

- Each enabled filesystem binding must declare an absolute `directory_path`.
- Successful filesystem publishes move the artifact from its current active
  location into the binding directory.
- Successful filesystem publishes persist the moved absolute path into
  `publish_runs.destination_ref`.

## Active artifact location

Artifact inspection surfaces expose the artifact's current active location.

- `runtime_artifact` means the artifact still resolves relative to the
  runtime-managed artifact output root.
- `filesystem_absolute` means a consuming filesystem publish moved the
  artifact and the active location now resolves through the persisted absolute
  host path.
- Process detail surfaces display both the active location kind and the active
  reference so operators can open the effective artifact path directly.

## Itch destination prerequisites

Itch destinations require destination-wide metadata and a ready credential.

- Destination config must include `account_name` and `game_slug`.
- Each bound build target must declare an Itch `channel`.
- Channels must not contain `:`.
- Operators can optionally provide an explicit `butler_path`. When it is
  empty, HGP resolves `butler` from the host `PATH`.
- Itch credentials use the `itch-api-key` secret kind.
- The desktop UI allows saving an Itch destination without a selected
  credential, but publish execution remains blocked until a ready credential is
  attached.

## Diagnostics expectations

Process detail and artifact inspection surfaces report publish destination
state per artifact.

- Artifact cards expose the current active location and publish counts.
- Artifact cards list each publish run with destination kind, destination
  name, status, and persisted destination reference when available.
- Repository detail summaries surface unbound build targets and credential
  gaps so operators can fix incomplete publish drafts before saving.
