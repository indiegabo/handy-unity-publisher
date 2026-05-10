package cli

import (
	"context"
	"database/sql"
	"flag"
	"fmt"
	"io"

	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/db"
	internalredis "github.com/indiegabo/handy-unity-bulder/internal/redis"
	redisv9 "github.com/redis/go-redis/v9"
)

// newCommandFlagSet builds a CLI flag set configured to write parse errors to
// the provided stderr stream.
func newCommandFlagSet(name string, stderr io.Writer) *flag.FlagSet {
	flagSet := flag.NewFlagSet(name, flag.ContinueOnError)
	flagSet.SetOutput(stderr)
	return flagSet
}

// withDatabaseAndRedis loads runtime config, opens SQLite and Redis, and then
// executes fn against those live dependencies.
func withDatabaseAndRedis(
	stderr io.Writer,
	fn func(context.Context, *sql.DB, *redisv9.Client) error,
) error {
	_ = stderr

	cfg, err := config.Load()
	if err != nil {
		return fmt.Errorf("load config: %w", err)
	}

	ctx := context.Background()
	database, err := db.Open(ctx, cfg)
	if err != nil {
		return fmt.Errorf("open database: %w", err)
	}
	defer database.Close()

	redisClient, err := internalredis.Open(ctx, cfg)
	if err != nil {
		return fmt.Errorf("open redis: %w", err)
	}
	defer redisClient.Close()

	return fn(ctx, database, redisClient)
}
