package pipelines

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"sort"
	"strings"

	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
	"github.com/indiegabo/handy-unity-bulder/internal/publish"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
)

// Synchronizer applies declarative manifests into the durable runtime stores.
type Synchronizer struct {
	credentials  credentials.Store
	repositories repository.Store
	builds       build.Store
	publishes    publish.Store
}

// ApplyStatus reports the outcome of one manifest application attempt.
type ApplyStatus struct {
	Path         string `json:"path"`
	PipelineName string `json:"pipeline_name"`
	Applied      bool   `json:"applied"`
	Error        string `json:"error,omitempty"`
}

// ApplyReport reports one full declarative synchronization round.
type ApplyReport struct {
	Pipelines []ApplyStatus `json:"pipelines"`
}

// NewSynchronizer creates a declarative synchronizer over the existing stores.
func NewSynchronizer(
	credentialsStore credentials.Store,
	repositoryStore repository.Store,
	buildStore build.Store,
	publishStore publish.Store,
) *Synchronizer {
	return &Synchronizer{
		credentials:  credentialsStore,
		repositories: repositoryStore,
		builds:       buildStore,
		publishes:    publishStore,
	}
}

// Apply synchronizes every manifest while preserving runtime history.
func (s *Synchronizer) Apply(
	ctx context.Context,
	manifests []Manifest,
	issues []LoadIssue,
) (ApplyReport, error) {
	report := ApplyReport{Pipelines: make([]ApplyStatus, 0, len(manifests)+len(issues))}
	for _, issue := range issues {
		report.Pipelines = append(report.Pipelines, ApplyStatus{
			Path:    issue.Path,
			Applied: false,
			Error:   issue.Error,
		})
	}

	if s == nil {
		return report, fmt.Errorf("pipeline synchronizer must not be nil")
	}

	existingCredentials, err := s.credentials.List(ctx)
	if err != nil {
		return report, fmt.Errorf("list credentials: %w", err)
	}
	credentialsByName := make(map[string]credentials.Record, len(existingCredentials))
	for _, record := range existingCredentials {
		credentialsByName[record.Name] = record
	}

	existingRepositories, err := s.repositories.List(ctx)
	if err != nil {
		return report, fmt.Errorf("list repositories: %w", err)
	}
	repositoriesByName := make(map[string]repository.Record, len(existingRepositories))
	for _, record := range existingRepositories {
		repositoriesByName[record.Name] = record
	}

	activeRepositories := make(map[string]struct{}, len(manifests))
	for _, manifest := range manifests {
		status := ApplyStatus{
			Path:         manifest.Path,
			PipelineName: manifest.Metadata.Name,
		}

		repoRecord, syncErr := s.applyManifest(
			ctx,
			manifest,
			credentialsByName,
			repositoriesByName,
		)
		if syncErr != nil {
			status.Error = syncErr.Error()
			report.Pipelines = append(report.Pipelines, status)
			continue
		}

		activeRepositories[repoRecord.Name] = struct{}{}
		status.Applied = true
		report.Pipelines = append(report.Pipelines, status)
	}

	repositoryNames := make([]string, 0, len(repositoriesByName))
	for name := range repositoriesByName {
		repositoryNames = append(repositoryNames, name)
	}
	sort.Strings(repositoryNames)

	for _, name := range repositoryNames {
		if _, ok := activeRepositories[name]; ok {
			continue
		}

		record := repositoriesByName[name]
		if !record.Enabled {
			continue
		}

		updated, err := s.repositories.Update(ctx, record.ID, repository.UpdateInput{
			Name:                   record.Name,
			RepoURL:                record.RepoURL,
			CredentialsID:          record.CredentialsID,
			DefaultBranch:          stringValue(record.DefaultBranch),
			PollingIntervalSeconds: record.PollingIntervalSeconds,
			Enabled:                false,
		})
		if err != nil {
			return report, fmt.Errorf("disable repository %q: %w", record.Name, err)
		}
		repositoriesByName[name] = updated
	}

	return report, nil
}

// applyManifest upserts one manifest and all of its dependent credentials,
// targets, and bindings.
func (s *Synchronizer) applyManifest(
	ctx context.Context,
	manifest Manifest,
	credentialsByName map[string]credentials.Record,
	repositoriesByName map[string]repository.Record,
) (repository.Record, error) {
	credentialIDs := make(map[string]int64, len(manifest.Spec.Credentials))
	for _, credentialSpec := range manifest.Spec.Credentials {
		record, err := s.upsertCredential(ctx, manifest, credentialSpec, credentialsByName)
		if err != nil {
			return repository.Record{}, err
		}

		credentialIDs[credentialSpec.Name] = record.ID
	}

	repoRecord, err := s.upsertRepository(ctx, manifest, repositoriesByName, credentialIDs)
	if err != nil {
		return repository.Record{}, err
	}

	buildTargets, err := s.syncBuildTargets(ctx, manifest, repoRecord, credentialIDs)
	if err != nil {
		return repository.Record{}, err
	}

	publishTargets, err := s.syncPublishTargets(ctx, manifest, repoRecord, credentialIDs)
	if err != nil {
		return repository.Record{}, err
	}

	if err := s.syncBindings(ctx, manifest, buildTargets, publishTargets); err != nil {
		return repository.Record{}, err
	}

	return repoRecord, nil
}

// upsertCredential resolves the manifest credential value sources and creates
// or updates the durable credentials record.
func (s *Synchronizer) upsertCredential(
	ctx context.Context,
	manifest Manifest,
	credentialSpec CredentialSpec,
	credentialsByName map[string]credentials.Record,
) (credentials.Record, error) {
	configJSON, err := buildCredentialConfigJSON(credentialSpec)
	if err != nil {
		return credentials.Record{}, fmt.Errorf(
			"sync credential %q in pipeline %q: %w",
			credentialSpec.Name,
			manifest.Metadata.Name,
			err,
		)
	}

	recordName := credentialRecordName(manifest.Metadata.Name, credentialSpec.Name)
	if existing, ok := credentialsByName[recordName]; ok {
		updated, err := s.credentials.Update(ctx, existing.ID, credentials.UpdateInput{
			Name:       recordName,
			Kind:       credentialSpec.Kind,
			ConfigJSON: configJSON,
		})
		if err != nil {
			return credentials.Record{}, fmt.Errorf("update credential %q: %w", recordName, err)
		}
		credentialsByName[recordName] = updated
		return updated, nil
	}

	created, err := s.credentials.Create(ctx, credentials.CreateInput{
		Name:       recordName,
		Kind:       credentialSpec.Kind,
		ConfigJSON: configJSON,
	})
	if err != nil {
		return credentials.Record{}, fmt.Errorf("create credential %q: %w", recordName, err)
	}
	credentialsByName[recordName] = created
	return created, nil
}

// upsertRepository creates or updates the durable repository row referenced by
// the manifest.
func (s *Synchronizer) upsertRepository(
	ctx context.Context,
	manifest Manifest,
	repositoriesByName map[string]repository.Record,
	credentialIDs map[string]int64,
) (repository.Record, error) {
	var credentialsID *int64
	if ref := strings.TrimSpace(manifest.Spec.Repository.Credentials); ref != "" {
		resolved, ok := credentialIDs[ref]
		if !ok {
			return repository.Record{}, fmt.Errorf(
				"pipeline %q references unknown repository credential %q",
				manifest.Metadata.Name,
				ref,
			)
		}
		credentialsID = &resolved
	}

	enabled := boolValue(manifest.Spec.Repository.Enabled, true)
	pollingInterval := manifest.Spec.Repository.PollingIntervalSeconds
	if pollingInterval == 0 {
		pollingInterval = 300
	}

	if existing, ok := repositoriesByName[manifest.Metadata.Name]; ok {
		updated, err := s.repositories.Update(ctx, existing.ID, repository.UpdateInput{
			Name:                   manifest.Metadata.Name,
			RepoURL:                manifest.Spec.Repository.URL,
			CredentialsID:          credentialsID,
			DefaultBranch:          manifest.Spec.Repository.DefaultBranch,
			PollingIntervalSeconds: pollingInterval,
			Enabled:                enabled,
		})
		if err != nil {
			return repository.Record{}, fmt.Errorf("update repository %q: %w", manifest.Metadata.Name, err)
		}
		repositoriesByName[manifest.Metadata.Name] = updated
		return updated, nil
	}

	created, err := s.repositories.Create(ctx, repository.CreateInput{
		Name:                   manifest.Metadata.Name,
		RepoURL:                manifest.Spec.Repository.URL,
		CredentialsID:          credentialsID,
		DefaultBranch:          manifest.Spec.Repository.DefaultBranch,
		PollingIntervalSeconds: pollingInterval,
		Enabled:                &enabled,
	})
	if err != nil {
		return repository.Record{}, fmt.Errorf("create repository %q: %w", manifest.Metadata.Name, err)
	}
	repositoriesByName[manifest.Metadata.Name] = created
	return created, nil
}

// syncBuildTargets upserts all declared build targets and disables any stale
// previously active targets omitted from the manifest.
func (s *Synchronizer) syncBuildTargets(
	ctx context.Context,
	manifest Manifest,
	repo repository.Record,
	_ map[string]int64,
) (map[string]build.Target, error) {
	existingTargets, err := s.builds.ListTargetsByRepository(ctx, repo.ID)
	if err != nil {
		return nil, fmt.Errorf("list build targets for repository %q: %w", repo.Name, err)
	}
	existingByName := make(map[string]build.Target, len(existingTargets))
	for _, target := range existingTargets {
		existingByName[target.Name] = target
	}

	activeNames := make(map[string]struct{}, len(manifest.Spec.Build.Targets))
	resolved := make(map[string]build.Target, len(manifest.Spec.Build.Targets))
	for _, targetSpec := range manifest.Spec.Build.Targets {
		configJSON, err := marshalJSONObject(targetSpec.Config)
		if err != nil {
			return nil, fmt.Errorf("marshal build target %q config: %w", targetSpec.Name, err)
		}

		enabled := boolValue(targetSpec.Enabled, true)
		input := build.UpdateTargetInput{
			Name:                 targetSpec.Name,
			Platform:             targetSpec.Platform,
			RunnerType:           runnerType(targetSpec.Runner.Type),
			BuildMethod:          targetSpec.BuildMethod,
			OutputKind:           targetSpec.Output.Kind,
			OutputPathTemplate:   targetSpec.Output.Path,
			UnityVersionOverride: targetSpec.Runner.UnityVersion,
			ImageOverride:        targetSpec.Runner.Image,
			TimeoutSeconds:       runnerTimeout(targetSpec.Runner.TimeoutSeconds),
			Enabled:              enabled,
			ConfigJSON:           configJSON,
		}

		var target build.Target
		if existing, ok := existingByName[targetSpec.Name]; ok {
			target, err = s.builds.UpdateTarget(ctx, existing.ID, input)
			if err != nil {
				return nil, fmt.Errorf("update build target %q: %w", targetSpec.Name, err)
			}
		} else {
			state := enabled
			target, err = s.builds.CreateTarget(ctx, build.CreateTargetInput{
				RepositoryID:         repo.ID,
				Name:                 input.Name,
				Platform:             input.Platform,
				RunnerType:           input.RunnerType,
				BuildMethod:          input.BuildMethod,
				OutputKind:           input.OutputKind,
				OutputPathTemplate:   input.OutputPathTemplate,
				UnityVersionOverride: input.UnityVersionOverride,
				ImageOverride:        input.ImageOverride,
				TimeoutSeconds:       input.TimeoutSeconds,
				Enabled:              &state,
				ConfigJSON:           input.ConfigJSON,
			})
			if err != nil {
				return nil, fmt.Errorf("create build target %q: %w", targetSpec.Name, err)
			}
		}

		activeNames[target.Name] = struct{}{}
		resolved[target.Name] = target
	}

	for _, existing := range existingTargets {
		if _, ok := activeNames[existing.Name]; ok || !existing.Enabled {
			continue
		}

		updated, err := s.builds.UpdateTarget(ctx, existing.ID, build.UpdateTargetInput{
			Name:                 existing.Name,
			Platform:             existing.Platform,
			RunnerType:           existing.RunnerType,
			BuildMethod:          stringValue(existing.BuildMethod),
			OutputKind:           stringValue(existing.OutputKind),
			OutputPathTemplate:   stringValue(existing.OutputPathTemplate),
			UnityVersionOverride: stringValue(existing.UnityVersionOverride),
			ImageOverride:        stringValue(existing.ImageOverride),
			TimeoutSeconds:       existing.TimeoutSeconds,
			Enabled:              false,
			ConfigJSON:           existing.ConfigJSON,
		})
		if err != nil {
			return nil, fmt.Errorf("disable build target %q: %w", existing.Name, err)
		}
		resolved[updated.Name] = updated
	}

	return resolved, nil
}

// syncPublishTargets upserts all declared publish targets and disables any
// stale previously active targets omitted from the manifest.
func (s *Synchronizer) syncPublishTargets(
	ctx context.Context,
	manifest Manifest,
	repo repository.Record,
	credentialIDs map[string]int64,
) (map[string]publish.Target, error) {
	existingTargets, err := s.publishes.ListTargetsByRepository(ctx, repo.ID)
	if err != nil {
		return nil, fmt.Errorf("list publish targets for repository %q: %w", repo.Name, err)
	}
	existingByName := make(map[string]publish.Target, len(existingTargets))
	for _, target := range existingTargets {
		existingByName[target.Name] = target
	}

	activeNames := make(map[string]struct{}, len(manifest.Spec.Publish.Targets))
	resolved := make(map[string]publish.Target, len(manifest.Spec.Publish.Targets))
	for _, targetSpec := range manifest.Spec.Publish.Targets {
		configJSON, err := marshalJSONObject(targetSpec.Config)
		if err != nil {
			return nil, fmt.Errorf("marshal publish target %q config: %w", targetSpec.Name, err)
		}

		var credentialsID *int64
		if ref := strings.TrimSpace(targetSpec.Credentials); ref != "" {
			resolvedID, ok := credentialIDs[ref]
			if !ok {
				return nil, fmt.Errorf(
					"pipeline %q references unknown publish credential %q for target %q",
					manifest.Metadata.Name,
					ref,
					targetSpec.Name,
				)
			}
			credentialsID = &resolvedID
		}

		enabled := boolValue(targetSpec.Enabled, true)
		input := publish.UpdateTargetInput{
			Name:          targetSpec.Name,
			Kind:          publishKind(targetSpec.Kind),
			CredentialsID: credentialsID,
			Enabled:       enabled,
			ConfigJSON:    configJSON,
		}

		var target publish.Target
		if existing, ok := existingByName[targetSpec.Name]; ok {
			target, err = s.publishes.UpdateTarget(ctx, existing.ID, input)
			if err != nil {
				return nil, fmt.Errorf("update publish target %q: %w", targetSpec.Name, err)
			}
		} else {
			state := enabled
			target, err = s.publishes.CreateTarget(ctx, publish.CreateTargetInput{
				RepositoryID:  repo.ID,
				Name:          input.Name,
				Kind:          input.Kind,
				CredentialsID: input.CredentialsID,
				Enabled:       &state,
				ConfigJSON:    input.ConfigJSON,
			})
			if err != nil {
				return nil, fmt.Errorf("create publish target %q: %w", targetSpec.Name, err)
			}
		}

		activeNames[target.Name] = struct{}{}
		resolved[target.Name] = target
	}

	for _, existing := range existingTargets {
		if _, ok := activeNames[existing.Name]; ok || !existing.Enabled {
			continue
		}

		updated, err := s.publishes.UpdateTarget(ctx, existing.ID, publish.UpdateTargetInput{
			Name:          existing.Name,
			Kind:          existing.Kind,
			CredentialsID: existing.CredentialsID,
			Enabled:       false,
			ConfigJSON:    existing.ConfigJSON,
		})
		if err != nil {
			return nil, fmt.Errorf("disable publish target %q: %w", existing.Name, err)
		}
		resolved[updated.Name] = updated
	}

	return resolved, nil
}

// syncBindings upserts all declared build-to-publish bindings and disables any
// stale bindings omitted from the manifest.
func (s *Synchronizer) syncBindings(
	ctx context.Context,
	manifest Manifest,
	buildTargets map[string]build.Target,
	publishTargets map[string]publish.Target,
) error {
	activeKeys := make(map[string]struct{}, len(manifest.Spec.Bindings))
	existingBindings := make(map[string]publish.Binding)

	for _, target := range buildTargets {
		bindings, err := s.publishes.ListBindingsByBuildTarget(ctx, target.ID)
		if err != nil {
			return fmt.Errorf("list bindings for build target %q: %w", target.Name, err)
		}
		for _, binding := range bindings {
			publishName := publishTargetNameByID(publishTargets, binding.PublishTargetID)
			if publishName == "" {
				continue
			}
			existingBindings[bindingKey(target.Name, publishName)] = binding
		}
	}

	for _, bindingSpec := range manifest.Spec.Bindings {
		buildTarget := buildTargets[bindingSpec.BuildTarget]
		publishTarget := publishTargets[bindingSpec.PublishTarget]
		optionsJSON, err := marshalJSONObject(bindingSpec.Options)
		if err != nil {
			return fmt.Errorf(
				"marshal binding %q -> %q options: %w",
				bindingSpec.BuildTarget,
				bindingSpec.PublishTarget,
				err,
			)
		}

		key := bindingKey(bindingSpec.BuildTarget, bindingSpec.PublishTarget)
		enabled := boolValue(bindingSpec.Enabled, true)
		if existing, ok := existingBindings[key]; ok {
			if _, err := s.publishes.UpdateBinding(ctx, existing.ID, publish.UpdateBindingInput{
				Enabled:     enabled,
				OptionsJSON: optionsJSON,
			}); err != nil {
				return fmt.Errorf("update binding %s: %w", key, err)
			}
		} else {
			state := enabled
			if _, err := s.publishes.CreateBinding(ctx, publish.CreateBindingInput{
				BuildTargetID:   buildTarget.ID,
				PublishTargetID: publishTarget.ID,
				Enabled:         &state,
				OptionsJSON:     optionsJSON,
			}); err != nil {
				return fmt.Errorf("create binding %s: %w", key, err)
			}
		}

		activeKeys[key] = struct{}{}
	}

	for key, existing := range existingBindings {
		if _, ok := activeKeys[key]; ok || !existing.Enabled {
			continue
		}

		if _, err := s.publishes.UpdateBinding(ctx, existing.ID, publish.UpdateBindingInput{
			Enabled:     false,
			OptionsJSON: existing.OptionsJSON,
		}); err != nil {
			return fmt.Errorf("disable binding %s: %w", key, err)
		}
	}

	return nil
}

// buildCredentialConfigJSON resolves one manifest credential into the JSON
// payload stored in the durable credentials table.
func buildCredentialConfigJSON(spec CredentialSpec) (string, error) {
	switch strings.ToLower(strings.TrimSpace(spec.Kind)) {
	case credentials.KindGitHTTPBasic:
		username, err := spec.Basic.Username.resolve()
		if err != nil {
			return "", err
		}
		password, err := spec.Basic.Password.resolve()
		if err != nil {
			return "", err
		}
		return marshalJSONObject(map[string]any{
			"username": username,
			"password": password,
		})
	case credentials.KindGitHTTPBearer:
		token, err := spec.Bearer.Token.resolve()
		if err != nil {
			return "", err
		}
		return marshalJSONObject(map[string]any{"token": token})
	default:
		return marshalJSONObject(spec.Config)
	}
}

// resolve materializes one value source from its literal, environment, or file
// input.
func (v ValueSource) resolve() (string, error) {
	if err := v.validateRequired(); err != nil {
		return "", err
	}

	if value := strings.TrimSpace(v.Value); value != "" {
		return value, nil
	}
	if name := strings.TrimSpace(v.Env); name != "" {
		value := strings.TrimSpace(os.Getenv(name))
		if value == "" {
			return "", fmt.Errorf("environment variable %q is empty", name)
		}
		return value, nil
	}

	path := strings.TrimSpace(v.File)
	contents, err := os.ReadFile(path)
	if err != nil {
		return "", fmt.Errorf("read file %q: %w", path, err)
	}
	value := strings.TrimSpace(string(contents))
	if value == "" {
		return "", fmt.Errorf("file %q is empty", path)
	}

	return value, nil
}

// credentialRecordName builds the durable credential name namespaced by the
// owning pipeline.
func credentialRecordName(pipelineName string, credentialName string) string {
	return strings.TrimSpace(pipelineName) + "/" + strings.TrimSpace(credentialName)
}

// marshalJSONObject encodes a map as canonical JSON, treating nil as an empty
// object.
func marshalJSONObject(value map[string]any) (string, error) {
	if value == nil {
		value = map[string]any{}
	}

	encoded, err := json.Marshal(value)
	if err != nil {
		return "", err
	}

	return string(encoded), nil
}

// stringValue dereferences a string pointer and returns the empty string for
// nil values.
func stringValue(value *string) string {
	if value == nil {
		return ""
	}

	return *value
}

// bindingKey builds the in-memory identifier used to deduplicate bindings by
// build target and publish target name.
func bindingKey(buildTargetName string, publishTargetName string) string {
	return strings.TrimSpace(buildTargetName) + "->" + strings.TrimSpace(publishTargetName)
}

// publishTargetNameByID resolves a publish target name from the in-memory map
// keyed by name.
func publishTargetNameByID(targets map[string]publish.Target, id int64) string {
	for name, target := range targets {
		if target.ID == id {
			return name
		}
	}

	return ""
}
