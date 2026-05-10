# syntax=docker/dockerfile:1.7

FROM golang:1.25-bookworm AS base

WORKDIR /workspace

ENV CGO_ENABLED=0 \
    GO111MODULE=on

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        docker.io \
        git && \
    rm -rf /var/lib/apt/lists/*

RUN mkdir -p /data /data/logs /data/artifacts /data/workspaces /workspace/tmp

FROM base AS dev

RUN --mount=type=cache,target=/root/.cache/go-build \
    --mount=type=cache,target=/go/pkg/mod \
    go install github.com/air-verse/air@latest

CMD ["air", "-c", ".air.toml"]

FROM base AS builder

COPY go.mod ./

RUN --mount=type=cache,target=/go/pkg/mod \
    go mod download

COPY . .

RUN --mount=type=cache,target=/root/.cache/go-build \
    --mount=type=cache,target=/go/pkg/mod \
    go build -o /out/server ./cmd/server && \
    go build -o /out/hgb ./cmd/hgb && \
    go build -o /out/build-worker ./cmd/build-worker && \
    go build -o /out/publish-worker ./cmd/publish-worker

FROM debian:bookworm-slim AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        docker.io \
		gosu \
        git && \
    rm -rf /var/lib/apt/lists/* && \
    useradd --create-home --shell /usr/sbin/nologin app && \
    mkdir -p /data /data/logs /data/artifacts /data/workspaces /var/lib/handy-unity-bulder

WORKDIR /app

COPY --from=builder /out/server /usr/local/bin/server
COPY --from=builder /out/hgb /usr/local/bin/hgb
COPY --from=builder /out/build-worker /usr/local/bin/build-worker
COPY --from=builder /out/publish-worker /usr/local/bin/publish-worker
COPY /scripts/runtime-entrypoint.sh /usr/local/bin/runtime-entrypoint

RUN chmod +x /usr/local/bin/runtime-entrypoint

ENV APP_ENV=production \
    HTTP_ADDR=:8080 \
    DATA_DIR=/data \
    LOG_LEVEL=info

EXPOSE 8080

ENTRYPOINT ["runtime-entrypoint"]
CMD ["server"]