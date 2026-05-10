// Command build-worker consumes queued build runs and executes them through
// the configured GameCI-compatible Docker path.
package main

import (
	"context"
	"fmt"
	"log/slog"
	"os"
	"os/signal"
	"syscall"

	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
	"github.com/indiegabo/handy-unity-bulder/internal/db"
	internaldocker "github.com/indiegabo/handy-unity-bulder/internal/docker"
	"github.com/indiegabo/handy-unity-bulder/internal/publish"
	internalredis "github.com/indiegabo/handy-unity-bulder/internal/redis"
	workerredis "github.com/indiegabo/handy-unity-bulder/internal/worker/redis"
)

// main runs the build worker bootstrap and exits with its process status code.
func main() {
	os.Exit(run())
}

// run opens runtime dependencies, assembles the build worker graph, and keeps
// consuming queued build jobs until shutdown.
func run() int {
	cfg, err := config.Load()
	if err != nil {
		_, _ = fmt.Fprintf(os.Stderr, "load config: %v\n", err)
		return 1
	}

	logger := slog.New(
		slog.NewTextHandler(
			os.Stdout,
			&slog.HandlerOptions{Level: cfg.SLogLevel()},
		),
	)

	ctx, stop := signal.NotifyContext(
		context.Background(),
		syscall.SIGINT,
		syscall.SIGTERM,
	)
	defer stop()

	database, err := db.Open(ctx, cfg)
	if err != nil {
		_, _ = fmt.Fprintf(os.Stderr, "open database: %v\n", err)
		return 1
	}
	defer database.Close()

	redisClient, err := internalredis.Open(ctx, cfg)
	if err != nil {
		_, _ = fmt.Fprintf(os.Stderr, "open redis: %v\n", err)
		return 1
	}
	defer redisClient.Close()

	queue := workerredis.NewQueue(redisClient)
	buildStore := build.NewStore(database)
	processor := build.NewExecutionProcessor(
		buildStore,
		build.NewWorkspacePreparer(cfg).WithCredentials(credentials.NewStore(database)),
		internaldocker.NewGameCIExecutor(),
	)
	publishCoordinator := publish.NewBuildResultDispatcher(
		publish.NewStore(database),
		publish.NewDispatcher(queue).WithCoordination(
			workerredis.NewLockManager(redisClient),
			workerredis.NewIdempotencyStore(redisClient),
		),
	)
	worker := build.NewWorker(
		buildStore,
		queue,
		processor,
	).WithPublishPlanner(publishCoordinator).WithLogger(logger)

	for {
		select {
		case <-ctx.Done():
			return 0
		default:
		}

		if _, err := worker.RunOnce(ctx); err != nil {
			logger.Error("run build worker", "error", err)
			return 1
		}
	}
}
