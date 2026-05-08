# Project Brief

## Project

Handy Unity Bulder

## Summary

Handy Unity Bulder is a self-hosted build orchestrator for Unity projects. The goal is to monitor registered repositories, detect new tagged releases, identify the Unity version required by each release, run builds with GameCI inside Docker, and publish the generated artifacts to configured distribution targets.

## Current Stage

The project is still in an early design and bootstrap phase. Right now, the repository mainly documents the intended direction of the product and its first architecture decisions.

## Main Goals

- Register Unity repositories with the credentials needed to access them
- Poll repositories for new tags
- Detect the Unity version associated with each tag
- Run isolated builds through Docker and GameCI
- Publish artifacts to targets such as Itch.io, Steam, and Google Drive
- Persist local state with SQLite on a Docker-mounted volume
- Provide a CLI for administration and operational workflows

## Initial Technical Direction

- Language: Go
- Database: SQLite
- Runtime: Docker on local WSL
- Build execution: ephemeral Docker containers managed by the application
- Persistence: SQLite database file stored on a mounted host volume

## Proposed Structure

The current direction points to a modular Go application with dedicated entrypoints for the server and CLI, plus internal modules for application flow, repository integration, build orchestration, persistence, publishing, and worker coordination.
