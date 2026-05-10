// Command poller runs one batch evaluation over enabled poll trigger rules and
// exits so operators can drive it from cron, tasks, or dedicated containers.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/signal"
	"syscall"

	internalgit "github.com/indiegabo/handy-unity-bulder/internal/git"
	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
	"github.com/indiegabo/handy-unity-bulder/internal/db"
	internalredis "github.com/indiegabo/handy-unity-bulder/internal/redis"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	"github.com/indiegabo/handy-unity-bulder/internal/trigger"
	workerredis "github.com/indiegabo/handy-unity-bulder/internal/worker/redis"
)

// main runs the polling sweep bootstrap and exits with its process status
// code.
func main() {
	os.Exit(run())
}

// run evaluates enabled polling rules once, writes the JSON report, and
// returns a non-zero status when the sweep fails or reports degraded rules.
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

	repositoryStore := repository.NewStore(database)
	triggerStore := trigger.NewStore(database)
	dispatcher := release.NewDispatcher(
		release.NewStore(database),
		workerredis.NewQueue(redisClient),
	).WithCoordination(
		workerredis.NewLockManager(redisClient),
		workerredis.NewIdempotencyStore(redisClient),
	)
	poller := release.NewPoller(
		repositoryStore,
		triggerStore,
		dispatcher,
		internalgit.NewRemoteTagSource(),
	).WithCredentials(credentials.NewStore(database))
	sweep := release.NewPollSweep(triggerStore, poller)

	report, err := sweep.RunOnce(ctx)
	if err != nil {
		_, _ = fmt.Fprintf(os.Stderr, "run poll sweep: %v\n", err)
		return 1
	}

	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(report); err != nil {
		_, _ = fmt.Fprintf(os.Stderr, "encode poll sweep report: %v\n", err)
		return 1
	}

	if report.HasFailures() {
		return 1
	}

	return 0
}