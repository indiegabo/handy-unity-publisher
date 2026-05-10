// Command publish-worker consumes queued publish runs and executes them through
// the configured publisher path.
package main

import (
	"context"
	"fmt"
	"os"
	"os/signal"
	"syscall"

	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/db"
	"github.com/indiegabo/handy-unity-bulder/internal/publish"
	internalredis "github.com/indiegabo/handy-unity-bulder/internal/redis"
	workerredis "github.com/indiegabo/handy-unity-bulder/internal/worker/redis"
)

// main runs the publish worker bootstrap and exits with its process status
// code.
func main() {
	os.Exit(run())
}

// run opens runtime dependencies, assembles the publish worker graph, and
// keeps consuming queued publish jobs until shutdown.
func run() int {
	cfg, err := config.Load()
	if err != nil {
		_, _ = fmt.Fprintf(os.Stderr, "load config: %v\n", err)
		return 1
	}

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

	worker := publish.NewWorker(
		publish.NewExecutionStore(database),
		workerredis.NewQueue(redisClient),
		publish.NewExecutionProcessor(),
	)

	for {
		select {
		case <-ctx.Done():
			return 0
		default:
		}

		if _, err := worker.RunOnce(ctx); err != nil {
			_, _ = fmt.Fprintf(os.Stderr, "run publish worker: %v\n", err)
			return 1
		}
	}
}