package hubcli

import (
	"context"
	"errors"
	"fmt"
	"io"
	"strconv"
	"strings"

	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
)

// dispatchUsage describes the positional hub command used to request one
// release by repository name and git tag.
const dispatchUsage = "hub dispatch requests one repository tag through the running HTTP API.\n\n" +
	"Usage:\n" +
	"  hub dispatch [--git-commit <sha>] [--rebuild] <repository> <git-tag>\n\n" +
	"Flags may be passed before or after the positional arguments.\n"

// dispatchCommandOptions captures the supported hub dispatch flags and
// positional arguments.
type dispatchCommandOptions struct {
	repositoryName string
	gitTag         string
	gitCommit      string
	rebuild        bool
}

// runDispatch resolves the named repository, posts a manual release dispatch,
// and prints the created release record.
func runDispatch(args []string, stdout, stderr io.Writer) int {
	options, err := parseDispatchArgs(args)
	if err != nil {
		_, _ = fmt.Fprintf(stderr, "%v\n\n%s", err, dispatchUsage)
		return 1
	}

	ctx := context.Background()
	client := newClient()
	if err := client.ensureAvailable(ctx); err != nil {
		_, _ = fmt.Fprintln(stderr, err)
		return 1
	}

	record, err := client.dispatchManualReleaseByRepositoryName(
		ctx,
		options.repositoryName,
		options.gitTag,
		options.gitCommit,
		options.rebuild,
	)
	if err != nil {
		_, _ = fmt.Fprintln(stderr, err)
		return 1
	}

	if err := printJSON(stdout, record); err != nil {
		_, _ = fmt.Fprintf(stderr, "encode release: %v\n", err)
		return 1
	}

	return 0
}

// parseDispatchArgs accepts dispatch flags before or after the positional
// repository and git tag arguments.
func parseDispatchArgs(args []string) (dispatchCommandOptions, error) {
	options := dispatchCommandOptions{}
	positionals := make([]string, 0, 2)

	for index := 0; index < len(args); index++ {
		argument := strings.TrimSpace(args[index])
		if argument == "" {
			continue
		}

		switch {
		case argument == "--rebuild":
			options.rebuild = true
		case strings.HasPrefix(argument, "--rebuild="):
			rebuild, err := strconv.ParseBool(
				strings.TrimSpace(strings.TrimPrefix(argument, "--rebuild=")),
			)
			if err != nil {
				return dispatchCommandOptions{}, fmt.Errorf(
					"invalid --rebuild value %q",
					strings.TrimSpace(strings.TrimPrefix(argument, "--rebuild=")),
				)
			}
			options.rebuild = rebuild
		case argument == "--git-commit":
			index++
			if index >= len(args) {
				return dispatchCommandOptions{}, fmt.Errorf(
					"--git-commit requires a value",
				)
			}

			options.gitCommit = strings.TrimSpace(args[index])
			if options.gitCommit == "" {
				return dispatchCommandOptions{}, fmt.Errorf(
					"--git-commit requires a non-empty value",
				)
			}
		case strings.HasPrefix(argument, "--git-commit="):
			options.gitCommit = strings.TrimSpace(
				strings.TrimPrefix(argument, "--git-commit="),
			)
			if options.gitCommit == "" {
				return dispatchCommandOptions{}, fmt.Errorf(
					"--git-commit requires a non-empty value",
				)
			}
		case strings.HasPrefix(argument, "-"):
			return dispatchCommandOptions{}, fmt.Errorf(
				"unknown flag %q",
				argument,
			)
		default:
			positionals = append(positionals, argument)
		}
	}

	if len(positionals) != 2 {
		return dispatchCommandOptions{}, fmt.Errorf(
			"expected <repository> and <git-tag>",
		)
	}

	options.repositoryName = strings.TrimSpace(positionals[0])
	options.gitTag = strings.TrimSpace(positionals[1])
	if options.repositoryName == "" || options.gitTag == "" {
		return dispatchCommandOptions{}, fmt.Errorf(
			"repository and git tag must not be empty",
		)
	}

	return options, nil
}

// findRepositoryByName resolves one repository name to its persisted record.
func findRepositoryByName(records []repository.Record, name string) (repository.Record, error) {
	for _, record := range records {
		if strings.EqualFold(strings.TrimSpace(record.Name), name) {
			return record, nil
		}
	}

	return repository.Record{}, fmt.Errorf("repository %q was not found", name)
}

// dispatchManualReleaseByRepositoryName resolves one repository name then
// dispatches the requested git tag over the HTTP API.
func (c *client) dispatchManualReleaseByRepositoryName(
	ctx context.Context,
	repositoryName string,
	gitTag string,
	gitCommit string,
	rebuild bool,
) (release.Record, error) {
	records, err := c.listRepositories(ctx)
	if err != nil {
		return release.Record{}, err
	}

	repo, err := findRepositoryByName(records, repositoryName)
	if err != nil {
		return release.Record{}, err
	}

	request := manualReleaseDispatchClientRequest{
		RepositoryID: repo.ID,
		GitTag:       gitTag,
		Rebuild:      rebuild,
	}
	if gitCommit != "" {
		request.GitCommit = gitCommit
	}

	releaseRecord, err := c.dispatchManualRelease(ctx, request)
	if err != nil {
		if errors.Is(err, repository.ErrNotFound) {
			return release.Record{}, fmt.Errorf("repository %q was not found", repositoryName)
		}
		return release.Record{}, err
	}

	return releaseRecord, nil
}
