package cli

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"io"

	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	workerredis "github.com/indiegabo/handy-unity-bulder/internal/worker/redis"
	redisv9 "github.com/redis/go-redis/v9"
)

// releaseUsage describes the top-level release management commands.
const releaseUsage = `hgb releases manages release dispatch and inspection.

Usage:
  hgb releases dispatch manual --repository-id <id> --git-tag <tag> [--git-commit <sha>]
	hgb releases plan --release-run-id <id>
`

// releaseDispatchUsage describes the release dispatch subcommands.
const releaseDispatchUsage = `hgb releases dispatch creates release work from a supported trigger source.

Usage:
  hgb releases dispatch manual --repository-id <id> --git-tag <tag> [--git-commit <sha>]
`

// releasePlanUsage describes the release planning command.
const releasePlanUsage = `hgb releases plan expands one queued release run into queued build runs.

Usage:
	hgb releases plan --release-run-id <id>
`

// runReleases routes the release subcommand tree.
func runReleases(args []string, stdout, stderr io.Writer) int {
	if len(args) == 0 {
		_, _ = fmt.Fprint(stderr, releaseUsage)
		return 1
	}

	switch args[0] {
	case "help", "-h", "--help":
		_, _ = fmt.Fprint(stdout, releaseUsage)
		return 0
	case "dispatch":
		return runReleaseDispatch(args[1:], stdout, stderr)
	case "plan":
		return runReleasePlan(args[1:], stdout, stderr)
	default:
		_, _ = fmt.Fprintf(stderr, "unknown releases command %q\n\n", args[0])
		_, _ = fmt.Fprint(stderr, releaseUsage)
		return 1
	}
}

// runReleaseDispatch routes the release dispatch subcommands.
func runReleaseDispatch(args []string, stdout, stderr io.Writer) int {
	if len(args) == 0 {
		_, _ = fmt.Fprint(stderr, releaseDispatchUsage)
		return 1
	}

	switch args[0] {
	case "help", "-h", "--help":
		_, _ = fmt.Fprint(stdout, releaseDispatchUsage)
		return 0
	case "manual":
		return runManualReleaseDispatch(args[1:], stdout, stderr)
	default:
		_, _ = fmt.Fprintf(stderr, "unknown releases dispatch command %q\n\n", args[0])
		_, _ = fmt.Fprint(stderr, releaseDispatchUsage)
		return 1
	}
}

// runManualReleaseDispatch validates CLI flags, persists a manual release run,
// and prints the created release record.
func runManualReleaseDispatch(args []string, stdout, stderr io.Writer) int {
	flagSet := newCommandFlagSet("releases dispatch manual", stderr)
	repositoryID := flagSet.Int64("repository-id", 0, "repository id")
	gitTag := flagSet.String("git-tag", "", "git tag to dispatch")
	gitCommit := flagSet.String("git-commit", "", "optional git commit sha")
	if err := flagSet.Parse(args); err != nil {
		return 1
	}

	if flagSet.NArg() != 0 {
		_, _ = fmt.Fprintln(stderr, "releases dispatch manual does not accept positional arguments")
		return 1
	}

	var record release.Record
	err := withDatabaseAndRedis(
		stderr,
		func(ctx context.Context, database *sql.DB, redisClient *redisv9.Client) error {
			store := release.NewStore(database)
			dispatcher := release.NewDispatcher(
				store,
				workerredis.NewQueue(redisClient),
			).WithCoordination(
				workerredis.NewLockManager(redisClient),
				workerredis.NewIdempotencyStore(redisClient),
			)

			created, err := dispatcher.DispatchManual(ctx, release.ManualDispatchInput{
				RepositoryID: *repositoryID,
				GitTag:       *gitTag,
				GitCommit:    *gitCommit,
				RequestedVia: "cli",
			})
			if err != nil {
				return err
			}

			record = created
			return nil
		},
	)
	if err != nil {
		switch {
		case isReleaseCommandError(err):
			_, _ = fmt.Fprintf(stderr, "%v\n", err)
		default:
			_, _ = fmt.Fprintf(stderr, "release command failed: %v\n", err)
		}
		return 1
	}

	if err := printJSON(stdout, record); err != nil {
		_, _ = fmt.Fprintf(stderr, "encode release: %v\n", err)
		return 1
	}

	return 0
}

// runReleasePlan expands one queued release into build runs, dispatches them,
// and prints the resulting build plan.
func runReleasePlan(args []string, stdout, stderr io.Writer) int {
	flagSet := newCommandFlagSet("releases plan", stderr)
	releaseRunID := flagSet.Int64("release-run-id", 0, "release run id")
	if err := flagSet.Parse(args); err != nil {
		return 1
	}

	if flagSet.NArg() != 0 || *releaseRunID <= 0 {
		_, _ = fmt.Fprint(stderr, releasePlanUsage)
		return 1
	}

	var runs []build.Run
	err := withDatabaseAndRedis(
		stderr,
		func(ctx context.Context, database *sql.DB, redisClient *redisv9.Client) error {
			store := build.NewStore(database)
			planned, err := store.PlanRelease(ctx, *releaseRunID)
			if err != nil {
				return err
			}

			dispatcher := build.NewDispatcher(
				workerredis.NewQueue(redisClient),
			).WithCoordination(
				workerredis.NewLockManager(redisClient),
				workerredis.NewIdempotencyStore(redisClient),
			)
			if err := dispatcher.DispatchMany(ctx, planned); err != nil {
				return err
			}

			runs = planned
			return nil
		},
	)
	if err != nil {
		switch {
		case isReleaseCommandError(err):
			_, _ = fmt.Fprintf(stderr, "%v\n", err)
		default:
			_, _ = fmt.Fprintf(stderr, "release command failed: %v\n", err)
		}
		return 1
	}

	if err := printJSON(stdout, runs); err != nil {
		_, _ = fmt.Fprintf(stderr, "encode build plan: %v\n", err)
		return 1
	}

	return 0
}

// isReleaseCommandError classifies store and dispatcher errors that should be
// written directly to stderr without extra wrapper text.
func isReleaseCommandError(err error) bool {
	return errors.Is(err, release.ErrInvalid) ||
		errors.Is(err, release.ErrNotFound) ||
		errors.Is(err, release.ErrConflict) ||
		errors.Is(err, release.ErrRepositoryNotFound) ||
		errors.Is(err, release.ErrDispatchInProgress) ||
		errors.Is(err, release.ErrDispatchAlreadyClaimed) ||
		errors.Is(err, build.ErrInvalid) ||
		errors.Is(err, build.ErrReleaseNotFound) ||
		errors.Is(err, build.ErrReleaseNotQueued) ||
		errors.Is(err, build.ErrNoEnabledTargets) ||
		errors.Is(err, build.ErrUnityVersionUnavailable) ||
		errors.Is(err, build.ErrImageResolutionUnavailable) ||
		errors.Is(err, build.ErrDispatchInProgress) ||
		errors.Is(err, build.ErrDispatchAlreadyClaimed) ||
		errors.Is(err, repository.ErrNotFound)
}
