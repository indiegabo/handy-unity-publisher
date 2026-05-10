package build

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
	internalgit "github.com/indiegabo/handy-unity-bulder/internal/git"
)

// PreparedWorkspace describes the filesystem layout allocated for one build
// run before Docker execution begins.
type PreparedWorkspace struct {
	RootPath             string `json:"root_path"`
	SourcePath           string `json:"source_path"`
	HostRootPath         string `json:"host_root_path"`
	HostSourcePath       string `json:"host_source_path"`
	LogPath              string `json:"log_path"`
	ArtifactRootPath     string `json:"artifact_root_path"`
	HostArtifactRootPath string `json:"host_artifact_root_path"`
}

// WorkspacePreparationInput defines the repository snapshot that must be
// materialized for one build run.
type WorkspacePreparationInput struct {
	BuildRunID              int64
	RepositoryName          string
	RepositoryURL           string
	RepositoryCredentialsID *int64
	GitTag                  string
}

// workspaceSyncer materializes the requested repository tag into the prepared
// source workspace.
type workspaceSyncer interface {
	SyncTag(
		ctx context.Context,
		repoURL string,
		workspacePath string,
		gitTag string,
		auth internalgit.AuthOptions,
	) error
}

// credentialLoader resolves stored repository credentials for authenticated
// workspace preparation.
type credentialLoader interface {
	Get(ctx context.Context, id int64) (credentials.Record, error)
}

// WorkspacePreparer allocates deterministic per-run directories and syncs the
// repository tag into the source workspace.
type WorkspacePreparer struct {
	workspacesDir     string
	logsDir           string
	artifactsDir      string
	hostWorkspacesDir string
	hostArtifactsDir  string
	credentials       credentialLoader
	syncer            workspaceSyncer
}

// NewWorkspacePreparer creates the default filesystem-backed workspace
// preparer using the configured runtime directories.
func NewWorkspacePreparer(cfg config.Config) *WorkspacePreparer {
	return &WorkspacePreparer{
		workspacesDir:     cfg.WorkspacesDir(),
		logsDir:           cfg.LogsDir(),
		artifactsDir:      cfg.ArtifactsDir(),
		hostWorkspacesDir: cfg.HostWorkspacesDir(),
		hostArtifactsDir:  cfg.HostArtifactsDir(),
		syncer:            internalgit.NewWorkspaceSyncer(),
	}
}

// WithCredentials configures the credentials loader used to authenticate Git
// workspace preparation for private repositories.
func (p *WorkspacePreparer) WithCredentials(loader credentialLoader) *WorkspacePreparer {
	p.credentials = loader
	return p
}

// newWorkspacePreparerWithSyncer injects a custom syncer for focused tests and
// alternate workspace preparation strategies.
func newWorkspacePreparerWithSyncer(
	cfg config.Config,
	syncer workspaceSyncer,
) *WorkspacePreparer {
	preparer := NewWorkspacePreparer(cfg)
	preparer.syncer = syncer
	return preparer
}

// Prepare creates deterministic per-run directories and checks out the
// requested repository tag into the isolated source workspace.
func (p *WorkspacePreparer) Prepare(
	ctx context.Context,
	input WorkspacePreparationInput,
) (PreparedWorkspace, error) {
	if input.BuildRunID <= 0 {
		return PreparedWorkspace{}, fmt.Errorf(
			"%w: build_run_id must be greater than zero",
			ErrInvalid,
		)
	}

	repositoryURL := strings.TrimSpace(input.RepositoryURL)
	if repositoryURL == "" {
		return PreparedWorkspace{}, fmt.Errorf(
			"%w: repository_url must not be empty",
			ErrInvalid,
		)
	}

	gitTag := strings.TrimSpace(input.GitTag)
	if gitTag == "" {
		return PreparedWorkspace{}, fmt.Errorf(
			"%w: git_tag must not be empty",
			ErrInvalid,
		)
	}

	prepared := PreparedWorkspace{
		RootPath: filepath.Join(
			filepath.Clean(p.workspacesDir),
			fmt.Sprintf("build-run-%d", input.BuildRunID),
		),
		HostRootPath: filepath.Join(
			filepath.Clean(p.hostWorkspacesDir),
			fmt.Sprintf("build-run-%d", input.BuildRunID),
		),
		LogPath: filepath.Join(
			filepath.Clean(p.logsDir),
			fmt.Sprintf("build-run-%d.log", input.BuildRunID),
		),
		ArtifactRootPath: filepath.Join(
			filepath.Clean(p.artifactsDir),
			artifactReleaseDirName(input.RepositoryName, repositoryURL, gitTag),
		),
		HostArtifactRootPath: filepath.Join(
			filepath.Clean(p.hostArtifactsDir),
			artifactReleaseDirName(input.RepositoryName, repositoryURL, gitTag),
		),
	}
	prepared.SourcePath = filepath.Join(prepared.RootPath, "source")
	prepared.HostSourcePath = filepath.Join(prepared.HostRootPath, "source")

	for _, directory := range []string{
		prepared.RootPath,
		filepath.Dir(prepared.LogPath),
		prepared.ArtifactRootPath,
	} {
		if err := os.MkdirAll(directory, 0o755); err != nil {
			return PreparedWorkspace{}, fmt.Errorf(
				"prepare build workspace directory %q: %w",
				directory,
				err,
			)
		}
	}

	if p.syncer == nil {
		return PreparedWorkspace{}, fmt.Errorf(
			"%w: workspace syncer is required",
			ErrInvalid,
		)
	}

	auth, err := p.resolveGitAuth(ctx, input.RepositoryCredentialsID)
	if err != nil {
		return PreparedWorkspace{}, err
	}

	if err := p.syncer.SyncTag(
		ctx,
		repositoryURL,
		prepared.SourcePath,
		gitTag,
		auth,
	); err != nil {
		return PreparedWorkspace{}, fmt.Errorf(
			"prepare build run %d source workspace: %w",
			input.BuildRunID,
			err,
		)
	}

	return prepared, nil
}

// resolveGitAuth resolves optional repository credentials into Git CLI auth
// flags for workspace synchronization.
func (p *WorkspacePreparer) resolveGitAuth(
	ctx context.Context,
	credentialsID *int64,
) (internalgit.AuthOptions, error) {
	if credentialsID == nil {
		return internalgit.AuthOptions{}, nil
	}
	if p.credentials == nil {
		return internalgit.AuthOptions{}, fmt.Errorf(
			"%w: workspace credentials loader is required",
			ErrInvalid,
		)
	}

	record, err := p.credentials.Get(ctx, *credentialsID)
	if err != nil {
		return internalgit.AuthOptions{}, fmt.Errorf(
			"load repository credentials %d: %w",
			*credentialsID,
			err,
		)
	}

	auth, err := internalgit.AuthOptionsFromCredentials(record)
	if err != nil {
		return internalgit.AuthOptions{}, fmt.Errorf(
			"resolve repository credentials %d for Git auth: %w",
			*credentialsID,
			err,
		)
	}

	return auth, nil
}
