# Project Brief

## Project Name

Handy Unity Bulder

## Summary

Handy Unity Bulder is intended to be a self-hosted build orchestrator for Unity projects. It should watch registered repositories for new tags, detect the Unity version needed for each release, execute builds with GameCI inside Docker, and publish the resulting artifacts to distribution targets.

## Current State

The repository is still in an early design and bootstrap phase. At the moment, the only committed project definition is the README, which captures the product goals and a proposed architecture.

## Main Objectives

- Register Unity repositories together with the credentials needed to access them
- Poll repositories for new tags
- Detect the Unity version associated with each tag
- Run isolated build jobs through Docker and GameCI
- Publish build outputs to platforms such as Itch.io, Steam, and Google Drive
- Persist application state locally with SQLite on a mounted Docker volume
- Provide a CLI for administration and day-to-day operations

## Initial Technical Direction

- Primary language: Go
- Local persistence: SQLite
- Runtime environment: Docker on local WSL
- Build workers: ephemeral Docker containers orchestrated by the application
- Persistent data requirement: SQLite file stored on a host-mounted Docker volume

## Expected High-Level Structure

The project is expected to grow around separate application, build, repository, publishing, persistence, and worker modules, with entrypoints for both the server and CLI.
