// Command server starts the main HTTP runtime for handy-unity-bulder.
package main

import (
	"context"
	"fmt"
	"log/slog"
	"os"
	"os/signal"
	"syscall"

	"github.com/indiegabo/handy-unity-bulder/internal/app"
	"github.com/indiegabo/handy-unity-bulder/internal/automation"
	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
	"github.com/indiegabo/handy-unity-bulder/internal/db"
	internalgit "github.com/indiegabo/handy-unity-bulder/internal/git"
	"github.com/indiegabo/handy-unity-bulder/internal/pipelines"
	"github.com/indiegabo/handy-unity-bulder/internal/publish"
	internalredis "github.com/indiegabo/handy-unity-bulder/internal/redis"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	"github.com/indiegabo/handy-unity-bulder/internal/trigger"
	"github.com/indiegabo/handy-unity-bulder/internal/version"
	workerredis "github.com/indiegabo/handy-unity-bulder/internal/worker/redis"
)

// main runs the server bootstrap and exits with its process status code.
func main() {
	os.Exit(run())
}

// run wires configuration, storage bootstrap, and graceful shutdown handling.
func run() int {
	cfg, err := config.Load()
	if err != nil {
		fmt.Fprintf(os.Stderr, "load config: %v\n", err)
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

	// The process refuses to serve requests until the persistent data layer is
	// ready, so migration failures surface immediately at startup.
	database, err := db.Open(ctx, cfg)
	if err != nil {
		logger.Error("bootstrap database", "error", err)
		return 1
	}
	defer func() {
		if err := database.Close(); err != nil {
			logger.Error("close database", "error", err)
		}
	}()

	logger.Info("database ready", "path", cfg.DBPath())

	// Redis stays outside the durable state model, but startup still validates
	// it because worker dispatch and coordination depend on it.
	redisClient, err := internalredis.Open(ctx, cfg)
	if err != nil {
		logger.Error("bootstrap redis", "error", err)
		return 1
	}
	defer func() {
		if err := redisClient.Close(); err != nil {
			logger.Error("close redis", "error", err)
		}
	}()

	logger.Info("redis ready", "addr", cfg.RedisAddr, "db", cfg.RedisDB)

	logger.Info(
		"starting server",
		"version", version.String(),
		"env", cfg.Env,
		"http_addr", cfg.HTTPAddr,
		"data_dir", cfg.DataDir,
		"redis_addr", cfg.RedisAddr,
	)

	credentialsStore := credentials.NewStore(database)
	repositoryStore := repository.NewStore(database)
	buildStore := build.NewStore(database)
	releaseStore := release.NewStore(database)
	triggerStore := trigger.NewStore(database)
	publishStore := publish.NewStore(database)
	pipelineLoaderResult, err := pipelines.LoadDir(cfg.PipelinesDir)
	if err != nil {
		logger.Error(
			"load declarative pipelines",
			"directory",
			cfg.PipelinesDir,
			"error",
			err,
		)
		return 1
	}

	pipelineReport, err := pipelines.NewSynchronizer(
		credentialsStore,
		repositoryStore,
		buildStore,
		publishStore,
	).Apply(ctx, pipelineLoaderResult.Manifests, pipelineLoaderResult.Issues)
	if err != nil {
		logger.Error("synchronize declarative pipelines", "error", err)
		return 1
	}
	for _, status := range pipelineReport.Pipelines {
		if status.Applied {
			logger.Info(
				"pipeline manifest applied",
				"pipeline",
				status.PipelineName,
				"path",
				status.Path,
			)
			continue
		}

		logger.Warn(
			"pipeline manifest skipped",
			"pipeline",
			status.PipelineName,
			"path",
			status.Path,
			"error",
			status.Error,
		)
	}

	queue := workerredis.NewQueue(redisClient)
	lockManager := workerredis.NewLockManager(redisClient)
	idempotencyStore := workerredis.NewIdempotencyStore(redisClient)

	releaseDispatcher := release.NewDispatcher(
		releaseStore,
		queue,
	).WithCoordination(lockManager, idempotencyStore)
	buildDispatcher := build.NewDispatcher(queue).WithCoordination(
		lockManager,
		idempotencyStore,
	)
	coordinator := automation.NewCoordinator(
		logger,
		repositoryStore,
		buildStore,
		credentialsStore,
		releaseStore,
		releaseDispatcher,
		buildDispatcher,
		queue,
		internalgit.NewRemoteTagSource(),
	).WithCoordination(lockManager)
	go func() {
		if err := coordinator.Run(ctx); err != nil {
			logger.Error("automation coordinator stopped with error", "error", err)
		}
	}()

	server := app.NewServer(
		cfg,
		logger,
		credentialsStore,
		repositoryStore,
		buildStore,
		releaseDispatcher,
		triggerStore,
		publishStore,
	).WithPipelineReport(pipelineReport).WithAutomationReporter(coordinator)
	if err := server.Run(ctx); err != nil {
		logger.Error("server stopped with error", "error", err)
		return 1
	}

	logger.Info("server stopped")
	return 0
}
