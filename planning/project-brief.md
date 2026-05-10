# Project Brief

Project name

Current repository name: handy-unity-bulder

Note: this name may contain a typo (bulder vs builder) and should be reviewed before the project grows.

Vision

Create a self-hosted tool that automates Unity game builds from Git repositories.

The tool should monitor registered repositories, detect new tags, build Unity projects automatically in Docker using GameCI-compatible images, and publish build artifacts to configured destinations such as Itch.io, Steam, and Google Drive.

The system should be simple enough to run locally in Docker, especially inside a WSL-based development workflow, while remaining extensible for future growth.

Problem to solve

Unity projects often require repetitive manual work to:

- monitor repositories for new release tags
- determine which Unity version is needed
- configure the correct build environment
- generate builds for multiple platforms
- gather artifacts different publication channels

This project centralizes and automates that process.

Primary goals

- register Unity repositories with credentials and polling rules
- detect new tags automatically
- identify the correct Unity version for each tag
- launch isolated build containers through Docker
- support multiple build targets per repository
- support multiple publication targets per repository
- allow explicit mapping between builds and publication targets
- store state locally with SQLite
- run the app itself inside Docker
- keep the solution lightweight and self-hosted

Non-goals for the initial phase

- full cloud SaaS platform
- distributed worker cluster
- complex multi-tenant permissions
- advanced analytics
- storing large build logs or artifacts inside the database
- overengineering for scale before it is needed

Core workflow

1. user registers a Unity repository
2. repository configuration includes:
   - Git access
   - polling settings
   - build targets
   - publication targets
   - bindings between them
3. the system polls for new tags
4. when a new tag is found:
   - a release run is created
   - build runs are created for enabled build targets
   - builds execute in Docker
   - artifacts are collected
   - publish runs are created from bindings
   - artifacts are published to their configured destinations
5. statuses are tracked for the whole release lifecycle

Key functional concepts

Repository
A registered Unity project plus its automation settings.

Build Target
A definition of a specific Unity build output for a repository.

Examples:
- Windows
- Linux
- WebGL
- macOS
- Android

Publish Target
A destination where built artifacts can be sent.

Examples:
- Itch.io
- Steam
- Google Drive

Binding
A link that tells the system which build target should publish to which publication target.

Release Run
A processing instance for a specific repository tag.

Build Run target one release.

Publish Run
An execution of one publication step for one built artifact or build output.

Technical direction

Language
- Go

Database
- SQLite initially

Persistence requirement
The SQLite database file must be persisted in a mounted Docker volume and be exportable/accesssible from the Docker host machine.

Runtime
- main app runs in Docker

Build strategy
- app controls Docker to launch ephemeral build containers
- Docker socket access is expected
- GameCI-compatible Unity build images are the initial strategy

File storage strategy
Database stores:
- configuration
- state
- metadata

Filesystem stores:
- logs
- artifacts
- workspaces

Suggested mounted data layout on.

Likely initial modules

cmd/
  server/
  hgb/

internal/
  app/
  build/
  cli/
  config/
  credentials/
  db/
  docker/
  git/
  publish/
  release/
  repository/
  worker/

Core data model direction

The initial schema is expected to include tables similar to:

- credentials
- repositories
- build_targets
- publish_targets
- build_publish_bindings
- release_runs
- build_runs
- artifacts
- publish_runs

Important design rule

A repository is not just a repo to watch.

A repository is a configurable release pipeline definition.

That means each repository should fully describe:
- what to build
- how to build
- where to publish
- which outputs go to which destinations

Initial target users

- indie developers
- small teams
- self-hosted Unity build workflows
- local/Wsl/Docker-based development users

Immediate next steps

1. finalize naming
2. create baseline docs
3. define SQLite schema
4. scaffold Go project
5. add Docker-based local runtime
6. implement repository configuration CRUD
7. implement polling and tag detection
8. implement build orchestration
9. implement publishing integrations