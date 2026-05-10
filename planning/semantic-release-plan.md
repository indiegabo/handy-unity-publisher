# Semantic Release Adoption Plan

## Objective

Establish one automated semantic versioning and release flow for the Go
application, its operator CLIs, and optional container artifacts without
manual version bumps in source files.

## Current State

- A shared version package already exists at
  `internal/version/version.go` with a `dev` fallback.
- `hgb`, the HTTP server, and health responses already read from that package.
- `hub` still prints a hardcoded `hub v0` string.
- The Makefile and Dockerfile build binaries without `ldflags` version
  injection.
- The repository does not yet have GitHub Actions workflows or release
  automation configuration.

## Recommended Tooling

- Use Conventional Commits as the release input contract.
- Use `go-semantic-release` to calculate the next version, create the Git tag,
  and publish GitHub release notes.
- Use GoReleaser to package binaries and optional Docker images after the
  version has been determined.
- Use GitHub Actions to enforce CI gates and execute the release workflow.

## Task List

### 1. Normalize the Version Source

- [ ] Replace the hardcoded `hub v0` output with the shared
      `internal/version` package.
- [ ] Decide the external format for CLI version output and keep it consistent
      across `hub`, `hgb`, server logs, and HTTP responses.
- [ ] Add focused tests that verify version output for the affected CLIs.
- [ ] Decide whether operator-facing build metadata also needs commit SHA,
      build date, or dirty state.

### 2. Make Local and Container Builds Version-Aware

- [ ] Introduce a `VERSION` variable in the Makefile with a `dev` default.
- [ ] Inject `VERSION` into every Go build with
      `-ldflags "-X github.com/indiegabo/handy-unity-bulder/internal/version.buildVersion=$(VERSION)"`.
- [ ] Apply the same version injection in the Dockerfile builder stage.
- [ ] Verify that local builds still report `dev` when `VERSION` is unset.
- [ ] Verify that explicit builds such as `VERSION=v1.2.3 make build` stamp the
      expected value into all produced binaries.

### 3. Establish CI Preconditions

- [ ] Add a GitHub Actions CI workflow for pushes and pull requests.
- [ ] Run formatting validation and the relevant Go test suites before any
      release step can execute.
- [ ] Ensure release-capable workflows fetch tags and full history instead of
      using shallow clones.
- [ ] Define the minimum repository permissions and secrets required for release
      publication.

### 4. Adopt Semantic Release Automation

- [ ] Configure Conventional Commit rules for `feat`, `fix`, and
      `BREAKING CHANGE` semantics.
- [ ] Add a release workflow triggered from the main release branch after CI
      succeeds.
- [ ] Configure `go-semantic-release` to inspect commits since the last tag,
      calculate the next SemVer, create the new tag, and publish release notes.
- [ ] Decide whether prereleases are needed for any non-main branches.
- [ ] Ensure the workflow fails without tagging when tests, packaging, or
      publication steps fail.

### 5. Package Release Artifacts

- [ ] Add a GoReleaser configuration for `hub`, `hgb`, `server`, `poller`,
      `build-worker`, and `publish-worker`.
- [ ] Publish versioned archives and checksums for the supported operating
      system targets.
- [ ] Decide whether Docker images are part of the same release scope.
- [ ] If Docker images are released, publish semver-tagged images and ensure
      the embedded binaries report the same version as the release tag.

### 6. Document the Operating Contract

- [ ] Document the Conventional Commit policy in the README or contribution
      guide.
- [ ] Document local dry-run commands for semantic-release and GoReleaser.
- [ ] Document how operators can inspect the running version from CLI and HTTP
      surfaces.
- [ ] Document rollback and recovery steps for a bad tag or partial release.

### 7. Validate End-to-End Release Behavior

- [ ] Run a dry release on a disposable branch or fork and verify the computed
      version matches the commit history.
- [ ] Run one real tagged release and verify the Git tag, GitHub release notes,
      packaged binaries, and optional Docker images all align.
- [ ] Verify that the released binaries return the same version through `hub
      version`, `hgb version`, server startup logs, and HTTP health responses.

## Acceptance Criteria

- [ ] Version numbers are derived from commit history rather than manual source
      edits.
- [ ] All binaries share the same injected build version.
- [ ] The release workflow creates exactly one SemVer tag per eligible commit
      range.
- [ ] Release artifacts are reproducible from the tagged source.
- [ ] Operator documentation explains both the commit discipline and the
      release execution path.

## Suggested Execution Order

1. Normalize the version source.
2. Make the build paths version-aware.
3. Add CI gates.
4. Add semantic-release automation.
5. Add GoReleaser packaging.
6. Document and dry-run the process.
7. Perform the first real release.