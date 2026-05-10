SHELL := bash
.SHELLFLAGS := -eu -o pipefail -c

.PHONY: help build fmt test test-focused dev-up dev-up-detached dev-down \
	hub-help hub-install hub-runtime-pipelines hub-runtime-automation \
	hub-dispatch hgb-config

help:
	@printf '%s\n' \
		'Available targets:' \
		'  make build' \
		'  make fmt' \
		'  make test' \
		'  make test-focused' \
		'  make dev-up' \
		'  make dev-up-detached' \
		'  make dev-down' \
		'  make hub-help' \
		'  make hub-install' \
		'  make hub-runtime-pipelines' \
		'  make hub-runtime-automation' \
		'  make hub-dispatch REPO=revolutions TAG=v1.0.0 [GIT_COMMIT=abcdef]' \
		'  make hgb-config'

build:
	mkdir -p ./bin
	go build -o ./bin/hub ./cmd/hub
	go build -o ./bin/hgb ./cmd/hgb
	go build -o ./bin/server ./cmd/server
	go build -o ./bin/poller ./cmd/poller
	go build -o ./bin/build-worker ./cmd/build-worker
	go build -o ./bin/publish-worker ./cmd/publish-worker

fmt:
	gofmt -w ./cmd ./internal ./e2e

test:
	go test ./...

test-focused:
	go test ./internal/release ./internal/automation ./internal/app ./internal/hubcli

dev-up:
	docker compose up --build

dev-up-detached:
	docker compose up -d --build

dev-down:
	docker compose down

hub-help:
	go run ./cmd/hub help

hub-install:
	go run ./cmd/hub install

hub-runtime-pipelines:
	go run ./cmd/hub runtime pipelines

hub-runtime-automation:
	go run ./cmd/hub runtime automation

hub-dispatch:
	if [[ -z "$${REPO:-}" || -z "$${TAG:-}" ]]; then \
		printf '%s\n' 'usage: make hub-dispatch REPO=revolutions TAG=v1.0.0 [GIT_COMMIT=abcdef] [REBUILD=1]'; \
		exit 1; \
	fi
	cmd=(go run ./cmd/hub dispatch "$${REPO}" "$${TAG}"); \
	if [[ -n "$${GIT_COMMIT:-}" ]]; then \
		cmd+=(--git-commit "$${GIT_COMMIT}"); \
	fi; \
	if [[ "$${REBUILD:-}" == "1" || "$${REBUILD:-}" == "true" ]]; then \
		cmd+=(--rebuild); \
	fi; \
	"$${cmd[@]}"

hgb-config:
	go run ./cmd/hgb config