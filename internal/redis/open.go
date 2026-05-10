// Package redis contains bootstrap helpers for transient coordination
// infrastructure used by workers and queue-driven flows.
package redis

import (
	"context"
	"fmt"
	"time"

	redisv9 "github.com/redis/go-redis/v9"

	"github.com/indiegabo/handy-unity-bulder/internal/config"
)

// Open connects to Redis and verifies the transient coordination layer is
// reachable before the application begins serving requests.
func Open(ctx context.Context, cfg config.Config) (*redisv9.Client, error) {
	client := redisv9.NewClient(&redisv9.Options{
		Addr:     cfg.RedisAddr,
		Username: cfg.RedisUsername,
		Password: cfg.RedisPassword,
		DB:       cfg.RedisDB,
	})

	pingCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()

	if err := client.Ping(pingCtx).Err(); err != nil {
		_ = client.Close()
		return nil, fmt.Errorf("ping redis: %w", err)
	}

	return client, nil
}
