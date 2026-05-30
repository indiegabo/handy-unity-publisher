# Semantic Release Plan

## Objective

Establish one automated semantic versioning and release flow for the Tauri
desktop application, bundled runtime, and published release artifacts without
manual version bumps in source files.

## Desired Outcome

Every release round should produce one SemVer version that is shared by:

- the desktop shell package metadata
- the bundled runtime version surfaces
- release notes and Git tags
- published desktop artifacts and checksums

## Current Baseline

- the repository already has a Cargo workspace and Tauri desktop shell
- the bundled runtime already exposes operator and diagnostics commands
- runtime and shell version surfaces need one explicit shared release contract
- `.github/workflows/ci.yml` now rejects deprecated build files and validates
      the active Rust and Tauri slices
- `.github/workflows/release-please.yml` now maintains release PRs and tags from
      Conventional Commit history on `main`
- `.github/workflows/release-bundle.yml` now builds and uploads the Windows
      desktop bundle plus checksums for published releases

## Recommended Release Model

- use Conventional Commits as the release input contract
- calculate the next SemVer from tags and commit history
- create the Git tag and GitHub release notes in the same release workflow
- build desktop artifacts from the tagged source
- ensure the packaged shell and bundled runtime report the same release version

## Release Artifact Scope

Each stable release should publish:

- platform-specific desktop bundles for the supported hosts
- checksums for the published artifacts
- GitHub release notes derived from the release commit range
- runtime and shell version metadata that can be inspected after installation

## Task List

### 1. Normalize The Version Source

- [ ] define one shared repository version source for the desktop shell and
      bundled runtime
- [ ] surface the resolved version in shell diagnostics and runtime contract
      outputs
- [ ] keep version formatting consistent across logs, status outputs, and the
      desktop settings UI
- [ ] decide whether build metadata also needs commit SHA, build date, or dirty
      state

### 2. Establish CI Preconditions

- [ ] add a GitHub Actions CI workflow for pushes and pull requests
- [ ] run formatting, compile, lint, and test gates before any release step can
      execute
- [ ] ensure release-capable workflows fetch tags and full history instead of
      shallow clones
- [ ] define the minimum repository permissions and secrets required for
      release publication

### 3. Adopt Semantic Release Automation

- [ ] configure Conventional Commit rules for `feat`, `fix`, and
      `BREAKING CHANGE` semantics
- [ ] add a release workflow triggered from the main release branch after CI
      succeeds
- [ ] calculate the next SemVer, create the new tag, and publish release notes
      from the same workflow
- [ ] decide whether prereleases are needed for non-main branches
- [ ] ensure the workflow fails without tagging when validation, packaging, or
      publication steps fail

### 4. Package Desktop Artifacts

- [ ] build the Tauri desktop bundles for the supported operating system
      targets
- [ ] publish versioned artifacts and checksums for each supported host
- [ ] ensure the bundled runtime version matches the release tag used for the
      desktop package
- [ ] verify that artifact names, release notes, and package metadata all carry
      the same version string

### 5. Document The Operating Contract

- [ ] document the Conventional Commit policy in the project documentation
- [ ] document local dry-run commands for the release workflow
- [ ] document how operators can inspect the running version from the desktop
      shell and runtime command surfaces
- [ ] document rollback and recovery steps for a bad tag or partial release

### 6. Validate End-To-End Release Behavior

- [ ] run a dry release on a disposable branch or fork and verify that the
      computed version matches the commit history
- [ ] run one real tagged release and verify the Git tag, GitHub release notes,
      desktop artifacts, and checksums all align
- [ ] verify that installed artifacts surface the expected version through shell
      diagnostics and runtime outputs

## Acceptance Criteria

- [ ] version numbers are derived from commit history rather than manual source
      edits
- [ ] shell and runtime surfaces report the same release version
- [ ] the release workflow creates exactly one SemVer tag per eligible commit
      range
- [ ] published artifacts are reproducible from the tagged source
- [ ] operator documentation explains the commit discipline and release
      execution path

## Suggested Execution Order

1. normalize the version source
2. add CI gates
3. add semantic release automation
4. package desktop artifacts
5. document and dry-run the process
6. perform the first real release
