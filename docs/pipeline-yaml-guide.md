# Pipeline YAML Guide

The repository configuration model is now declarative.

Each repository pipeline lives in one YAML file under the repository-root
`pipelines/` directory. The YAML files are the source of truth for:

- repository access
- repository polling cadence
- credentials references
- build targets
- publish targets
- build to publish bindings

The runtime loads every `.yml` or `.yaml` file from `pipelines/` during server
startup. Valid files are synchronized into the runtime state store before the
automation coordinator starts. Invalid files are logged and skipped without
blocking valid pipelines.

In local development, the Compose-backed `unity-build-api` process runs
through `air`. Because the dev watcher includes `.yml` and `.yaml` files,
changing manifest contents restarts the server and reapplies the pipeline set.
Production-style runtimes still apply manifests during process startup only.

`hub runtime pipelines` returns the last startup synchronization report.

`hub runtime automation` returns the live per-repository automation state,
including whether polling is paused by an active build backlog and which tags
are still queued behind the current release.

Important state model:

- YAML is the configuration source of truth.
- SQLite is still used for durable runtime state such as releases, build runs,
  publish runs, artifacts, and synchronization metadata.
- `hub db export` and `hub db import` remain the supported snapshot flows.

## Polling Execution Semantics

The synchronized manifest controls whether the repository participates in
automatic polling, but the runtime processes the resulting work in a strict
repository-local sequence:

- only enabled repositories with at least one enabled build target are polled
- one polling pass queues every unseen tag after `last_seen_tag`, from the
  oldest unseen tag to the newest unseen tag
- the runtime only starts one release build process per repository at a time
- one release build process means all enabled build targets for one Git tag
- polling for that repository stays paused while the current release still has
  queued or running build work
- the next queued tag only starts after every build target of the current
  release has reached a terminal status, even if some targets failed

Use `hub runtime automation` when you need to confirm whether a repository is
currently ready to poll, merely waiting for its next interval, or paused by a
release backlog.

## Directory Contract

Local development uses:

```text
pipelines/
```

inside the repository root.

Manifest files in that directory are git-ignored on purpose. They are treated
as local runtime inputs, not committed application code.

The server resolves that directory from `PIPELINES_DIR`. In Docker Compose the
default value is `/workspace/pipelines`, which maps to the repository-root
`pipelines/` directory through the existing workspace mount.

## Minimal Rules

- one repository pipeline per file
- `metadata.name` must be unique across all files in `pipelines/`
- use `.yml` or `.yaml`
- use environment variables or files for secrets when possible
- do not rely on `hub` or `hgb` to create configuration records; those CRUD
  flows are no longer the supported path

## Supported Declarative Features

Current first-class credential helpers:

- `git-http-basic`
- `git-http-bearer`

Current first-class publish execution kind:

- `filesystem`

Other credential kinds can still be stored through generic `config`, but only
the features above have dedicated declarative helpers and runtime behavior in
this version.

## Full Example

```yaml
apiVersion: handy.unity.builder/v1alpha1
kind: Pipeline

metadata:
  name: revolutions

spec:
  repository:
    url: https://github.com/indiegabo/revolutions.git
    defaultBranch: main
    enabled: true
    pollingIntervalSeconds: 300
    credentials: origin

  credentials:
    - name: origin
      kind: git-http-basic
      basic:
        username:
          env: REVOLUTIONS_GIT_USERNAME
        password:
          env: REVOLUTIONS_GIT_TOKEN

  build:
    targets:
      - name: linux64
        enabled: true
        platform: StandaloneLinux64
        buildMethod: Builder.BuildLinux64
        runner:
          type: gameci
          unityVersion: 2022.3.14f1
          timeoutSeconds: 5400
        output:
          kind: archive
          path: Builds/Linux64
        config:
          compression: zip

      - name: webgl
        enabled: true
        platform: WebGL
        buildMethod: Builder.BuildWebGL
        runner:
          type: gameci
          unityVersion: 2022.3.14f1
          timeoutSeconds: 5400
        output:
          kind: directory
          path: Builds/WebGL
        config: {}

  publish:
    targets:
      - name: filesystem-release
        enabled: true
        kind: filesystem
        config:
          root_path: /data/published

  bindings:
    - buildTarget: linux64
      publishTarget: filesystem-release
      enabled: true
      options:
        channel: stable

    - buildTarget: webgl
      publishTarget: filesystem-release
      enabled: true
      options: {}
```

The `output.path` values above describe the requested build-method path shape,
not the final operator-facing artifact filename. The runtime stores the final
files under canonical names before execution starts. For `output.kind:
archive`, the requested path must not end with `.zip`; use a staging path such
as `Builds/WebGL` or `Builds/Linux64` instead.

## Template

Use this as the starting point for a new repository pipeline:

```yaml
apiVersion: handy.unity.builder/v1alpha1
kind: Pipeline

metadata:
  name: <pipeline-name>

spec:
  repository:
    url: <git-url>
    defaultBranch: <branch>
    enabled: true
    pollingIntervalSeconds: 300
    credentials: <credential-name-or-empty>

  credentials:
    - name: <credential-name>
      kind: git-http-basic
      basic:
        username:
          env: <ENV_VAR_FOR_USERNAME>
        password:
          env: <ENV_VAR_FOR_PASSWORD_OR_TOKEN>

  build:
    targets:
      - name: <target-name>
        enabled: true
        platform: <unity-platform>
        buildMethod: <static-unity-method>
        runner:
          type: gameci
          unityVersion: <unity-version-or-empty>
          image: <image-override-or-empty>
          timeoutSeconds: 3600
        output:
          kind: <archive-or-directory>
          path: <relative-requested-build-path>
        config: {}

  publish:
    targets:
      - name: <publish-target-name>
        enabled: true
        kind: filesystem
        credentials: <credential-name-or-empty>
        config:
          root_path: <absolute-destination-path>

  bindings:
    - buildTarget: <target-name>
      publishTarget: <publish-target-name>
      enabled: true
      options: {}
```

## Value Sources

Credential helpers can resolve a value from exactly one source:

Literal value:

```yaml
password:
  value: ghp_example
```

Environment variable:

```yaml
password:
  env: REVOLUTIONS_GIT_TOKEN
```

File path:

```yaml
password:
  file: /run/secrets/revolutions_git_token
```

Do not set more than one of `value`, `env`, or `file` on the same field.

## Field Notes

- `metadata.name` becomes the durable repository name.
- `spec.repository.credentials` references one entry from `spec.credentials`.
- `spec.build.targets[].runner.type` defaults to `gameci`.
- `spec.build.targets[].runner.timeoutSeconds` defaults to the runtime build
  timeout when omitted or zero.
- `spec.build.targets[].output.path` is an execution hint for the Unity build
  method. The runtime uses it to preserve artifact style or extension when
  needed, but it rewrites the final stored artifact name.
- `spec.build.targets[].output.path` must not end with `.zip` when
  `spec.build.targets[].output.kind` is `archive`. Archive naming is canonical
  and owned by the runtime.
- `spec.build.targets[].config`, `spec.publish.targets[].config`, and
  `spec.bindings[].options` are stored as JSON objects for executor-specific
  behavior.
- `spec.publish.targets[].config.root_path` must be absolute for `filesystem`
  publish targets.

## Artifact Naming And Storage

The runtime stores build outputs under a canonical release directory:

```text
artifacts/<metadata.name>.<git-tag>/
```

Inside that directory, each build target is normalized to:

```text
<metadata.name>.<git-tag>.<build-target><ext>
```

The `<metadata.name>` portion is converted into a slug before storage:

- lowercase only
- spaces become `-`
- accents and other special characters are removed

Example:

```text
Meu Repositório -> meu-repositorio
```

Naming rules:

- archive targets always end with `.zip`
- non-archive targets keep only the extension implied by `output.path`, when
  one exists
- directory-style targets keep the canonical basename without preserving the
  original relative path from YAML
- archive targets still require an `output.path`, but that path is only a
  Unity-side requested staging path and must not try to name the final zip file
- reruns of the same repository, tag, and target replace the previous output
  at that canonical path

That means `output.path` should be treated as a build-method hint, not as the
final operator-visible filename on the host.

## AI Agent Questionnaire

When an AI agent is asked to create one pipeline YAML, it should gather the
information in this order and only then write the file:

1. Ask for the pipeline name.
2. Ask for the Git repository URL.
3. Ask for the default branch.
4. Ask whether the pipeline is enabled.
5. Ask for the polling interval in seconds.
6. Ask whether Git credentials are required.
7. If credentials are required, ask which kind applies:
   - `git-http-basic`
   - `git-http-bearer`
8. Ask whether each secret should come from an environment variable, a file,
   or an explicit literal value.
9. Ask for build targets one by one:
   - target name
   - Unity platform
   - Unity static build method
   - output kind
  - output path hint or expected extension
  - for archive outputs, ask for a staging path without a `.zip` suffix
   - Unity version override if any
   - image override if any
   - timeout
   - optional config object
10. Ask for publish targets one by one.
11. If the user asks for a publish kind other than `filesystem`, explain that
    only `filesystem` is currently executed by the publish worker.
12. Ask for the bindings between build targets and publish targets.
13. Ask where the resulting file should be written under `pipelines/`.
14. Generate the YAML only after the questionnaire is complete.

The agent should not guess missing `buildMethod`, `platform`, or secret source
values.