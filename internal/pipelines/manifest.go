// Package pipelines loads declarative repository pipeline manifests from YAML.
package pipelines

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"gopkg.in/yaml.v3"

	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
	"github.com/indiegabo/handy-unity-bulder/internal/publish"
)

const (
	// APIVersion is the manifest version accepted by the V1 declarative loader.
	APIVersion = "handy.unity.builder/v1alpha1"
	// Kind is the manifest kind accepted by the V1 declarative loader.
	Kind = "Pipeline"
)

// Manifest describes one repository pipeline declared through YAML.
type Manifest struct {
	APIVersion string   `yaml:"apiVersion"`
	Kind       string   `yaml:"kind"`
	Metadata   Metadata `yaml:"metadata"`
	Spec       Spec     `yaml:"spec"`
	Path       string   `yaml:"-"`
	FileName   string   `yaml:"-"`
}

// Metadata identifies one manifest uniquely inside the declarative directory.
type Metadata struct {
	Name string `yaml:"name"`
}

// Spec contains the pipeline configuration declared by one manifest.
type Spec struct {
	Repository  RepositorySpec   `yaml:"repository"`
	Credentials []CredentialSpec `yaml:"credentials"`
	Build       BuildSpec        `yaml:"build"`
	Publish     PublishSpec      `yaml:"publish"`
	Bindings    []BindingSpec    `yaml:"bindings"`
}

// RepositorySpec defines the Git repository and polling behavior.
type RepositorySpec struct {
	URL                    string `yaml:"url"`
	DefaultBranch          string `yaml:"defaultBranch"`
	Enabled                *bool  `yaml:"enabled"`
	PollingIntervalSeconds int    `yaml:"pollingIntervalSeconds"`
	Credentials            string `yaml:"credentials"`
}

// CredentialSpec defines one named credential that other sections can reference.
type CredentialSpec struct {
	Name   string                `yaml:"name"`
	Kind   string                `yaml:"kind"`
	Basic  *BasicCredentialSpec  `yaml:"basic"`
	Bearer *BearerCredentialSpec `yaml:"bearer"`
	Config map[string]any        `yaml:"config"`
}

// BasicCredentialSpec defines a git-http-basic credential.
type BasicCredentialSpec struct {
	Username ValueSource `yaml:"username"`
	Password ValueSource `yaml:"password"`
}

// BearerCredentialSpec defines a git-http-bearer credential.
type BearerCredentialSpec struct {
	Token ValueSource `yaml:"token"`
}

// ValueSource resolves one string either from a literal value, an environment
// variable, or a file path.
type ValueSource struct {
	Value string `yaml:"value"`
	Env   string `yaml:"env"`
	File  string `yaml:"file"`
}

// BuildSpec defines all declared build targets for one repository.
type BuildSpec struct {
	Targets []BuildTargetSpec `yaml:"targets"`
}

// BuildTargetSpec defines one Unity build target.
type BuildTargetSpec struct {
	Name        string         `yaml:"name"`
	Enabled     *bool          `yaml:"enabled"`
	Platform    string         `yaml:"platform"`
	BuildMethod string         `yaml:"buildMethod"`
	Runner      RunnerSpec     `yaml:"runner"`
	Output      OutputSpec     `yaml:"output"`
	Config      map[string]any `yaml:"config"`
}

// RunnerSpec describes the build runner overrides for one target.
type RunnerSpec struct {
	Type           string `yaml:"type"`
	UnityVersion   string `yaml:"unityVersion"`
	Image          string `yaml:"image"`
	TimeoutSeconds int    `yaml:"timeoutSeconds"`
}

// OutputSpec describes the expected build artifact location.
type OutputSpec struct {
	Kind string `yaml:"kind"`
	Path string `yaml:"path"`
}

// PublishSpec defines the publish targets declared for one repository.
type PublishSpec struct {
	Targets []PublishTargetSpec `yaml:"targets"`
}

// PublishTargetSpec defines one publish destination.
type PublishTargetSpec struct {
	Name        string         `yaml:"name"`
	Enabled     *bool          `yaml:"enabled"`
	Kind        string         `yaml:"kind"`
	Credentials string         `yaml:"credentials"`
	Config      map[string]any `yaml:"config"`
}

// BindingSpec declares one build target to publish target connection.
type BindingSpec struct {
	BuildTarget   string         `yaml:"buildTarget"`
	PublishTarget string         `yaml:"publishTarget"`
	Enabled       *bool          `yaml:"enabled"`
	Options       map[string]any `yaml:"options"`
}

// LoadIssue reports one file that could not be decoded or validated.
type LoadIssue struct {
	Path  string `json:"path"`
	Error string `json:"error"`
}

// LoadResult captures the valid manifests and invalid files observed during one load.
type LoadResult struct {
	Manifests []Manifest  `json:"manifests"`
	Issues    []LoadIssue `json:"issues"`
}

// LoadDir reads every .yml or .yaml manifest file from one directory.
func LoadDir(dir string) (LoadResult, error) {
	cleanDir := filepath.Clean(strings.TrimSpace(dir))
	if cleanDir == "" {
		return LoadResult{}, fmt.Errorf("pipelines directory must not be empty")
	}

	entries, err := os.ReadDir(cleanDir)
	if err != nil {
		if os.IsNotExist(err) {
			return LoadResult{}, nil
		}

		return LoadResult{}, fmt.Errorf("read pipelines directory %q: %w", cleanDir, err)
	}

	files := make([]string, 0, len(entries))
	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}

		name := strings.TrimSpace(entry.Name())
		if name == "" || strings.HasPrefix(name, ".") {
			continue
		}

		ext := strings.ToLower(filepath.Ext(name))
		if ext != ".yml" && ext != ".yaml" {
			continue
		}

		files = append(files, filepath.Join(cleanDir, name))
	}
	sort.Strings(files)

	result := LoadResult{
		Manifests: make([]Manifest, 0, len(files)),
		Issues:    make([]LoadIssue, 0),
	}
	seenNames := make(map[string]string, len(files))

	for _, path := range files {
		manifest, err := loadManifest(path)
		if err != nil {
			result.Issues = append(result.Issues, LoadIssue{Path: path, Error: err.Error()})
			continue
		}

		if firstPath, ok := seenNames[manifest.Metadata.Name]; ok {
			result.Issues = append(result.Issues, LoadIssue{
				Path:  path,
				Error: fmt.Sprintf("duplicate metadata.name %q already declared by %s", manifest.Metadata.Name, firstPath),
			})
			continue
		}

		seenNames[manifest.Metadata.Name] = path
		result.Manifests = append(result.Manifests, manifest)
	}

	return result, nil
}

// loadManifest reads, decodes, and validates one manifest file.
func loadManifest(path string) (Manifest, error) {
	contents, err := os.ReadFile(path)
	if err != nil {
		return Manifest{}, fmt.Errorf("read manifest %q: %w", path, err)
	}

	decoder := yaml.NewDecoder(strings.NewReader(string(contents)))
	decoder.KnownFields(true)

	var manifest Manifest
	if err := decoder.Decode(&manifest); err != nil {
		return Manifest{}, fmt.Errorf("decode manifest %q: %w", path, err)
	}

	manifest.Path = path
	manifest.FileName = filepath.Base(path)
	if err := manifest.Validate(); err != nil {
		return Manifest{}, fmt.Errorf("validate manifest %q: %w", path, err)
	}

	return manifest, nil
}

// Validate checks one manifest for the minimum invariants required by the runtime.
func (m Manifest) Validate() error {
	if strings.TrimSpace(m.APIVersion) != APIVersion {
		return fmt.Errorf("apiVersion must be %q", APIVersion)
	}
	if strings.TrimSpace(m.Kind) != Kind {
		return fmt.Errorf("kind must be %q", Kind)
	}
	if strings.TrimSpace(m.Metadata.Name) == "" {
		return fmt.Errorf("metadata.name must not be empty")
	}
	if strings.TrimSpace(m.Spec.Repository.URL) == "" {
		return fmt.Errorf("spec.repository.url must not be empty")
	}
	if m.Spec.Repository.PollingIntervalSeconds < 0 {
		return fmt.Errorf("spec.repository.pollingIntervalSeconds must not be negative")
	}

	credentialsByName := make(map[string]struct{}, len(m.Spec.Credentials))
	for _, credential := range m.Spec.Credentials {
		name := strings.TrimSpace(credential.Name)
		if name == "" {
			return fmt.Errorf("spec.credentials[].name must not be empty")
		}
		if _, ok := credentialsByName[name]; ok {
			return fmt.Errorf("spec.credentials contains duplicate name %q", name)
		}
		credentialsByName[name] = struct{}{}

		kind := strings.ToLower(strings.TrimSpace(credential.Kind))
		if kind == "" {
			return fmt.Errorf("spec.credentials[%q].kind must not be empty", name)
		}

		switch kind {
		case credentials.KindGitHTTPBasic:
			if credential.Basic == nil {
				return fmt.Errorf("spec.credentials[%q].basic is required for %q", name, credentials.KindGitHTTPBasic)
			}
			if err := credential.Basic.Username.validateRequired(); err != nil {
				return fmt.Errorf("spec.credentials[%q].basic.username: %w", name, err)
			}
			if err := credential.Basic.Password.validateRequired(); err != nil {
				return fmt.Errorf("spec.credentials[%q].basic.password: %w", name, err)
			}
		case credentials.KindGitHTTPBearer:
			if credential.Bearer == nil {
				return fmt.Errorf("spec.credentials[%q].bearer is required for %q", name, credentials.KindGitHTTPBearer)
			}
			if err := credential.Bearer.Token.validateRequired(); err != nil {
				return fmt.Errorf("spec.credentials[%q].bearer.token: %w", name, err)
			}
		default:
			if credential.Config == nil {
				credential.Config = map[string]any{}
			}
		}
	}

	if ref := strings.TrimSpace(m.Spec.Repository.Credentials); ref != "" {
		if _, ok := credentialsByName[ref]; !ok {
			return fmt.Errorf("spec.repository.credentials references unknown credential %q", ref)
		}
	}

	buildTargets := make(map[string]struct{}, len(m.Spec.Build.Targets))
	for _, target := range m.Spec.Build.Targets {
		name := strings.TrimSpace(target.Name)
		if name == "" {
			return fmt.Errorf("spec.build.targets[].name must not be empty")
		}
		if _, ok := buildTargets[name]; ok {
			return fmt.Errorf("spec.build.targets contains duplicate name %q", name)
		}
		buildTargets[name] = struct{}{}

		if strings.TrimSpace(target.Platform) == "" {
			return fmt.Errorf("spec.build.targets[%q].platform must not be empty", name)
		}
		if strings.TrimSpace(target.BuildMethod) == "" {
			return fmt.Errorf("spec.build.targets[%q].buildMethod must not be empty", name)
		}
		if strings.TrimSpace(target.Output.Kind) == "" {
			return fmt.Errorf("spec.build.targets[%q].output.kind must not be empty", name)
		}
		if strings.TrimSpace(target.Output.Path) == "" {
			return fmt.Errorf("spec.build.targets[%q].output.path must not be empty", name)
		}
		if err := build.ValidateRequestedOutputPath(target.Output.Kind, target.Output.Path); err != nil {
			return fmt.Errorf("spec.build.targets[%q].output.path: %w", name, err)
		}
		if target.Runner.TimeoutSeconds < 0 {
			return fmt.Errorf("spec.build.targets[%q].runner.timeoutSeconds must not be negative", name)
		}
	}

	publishTargets := make(map[string]struct{}, len(m.Spec.Publish.Targets))
	for _, target := range m.Spec.Publish.Targets {
		name := strings.TrimSpace(target.Name)
		if name == "" {
			return fmt.Errorf("spec.publish.targets[].name must not be empty")
		}
		if _, ok := publishTargets[name]; ok {
			return fmt.Errorf("spec.publish.targets contains duplicate name %q", name)
		}
		publishTargets[name] = struct{}{}

		if strings.TrimSpace(target.Kind) == "" {
			return fmt.Errorf("spec.publish.targets[%q].kind must not be empty", name)
		}
		if ref := strings.TrimSpace(target.Credentials); ref != "" {
			if _, ok := credentialsByName[ref]; !ok {
				return fmt.Errorf("spec.publish.targets[%q].credentials references unknown credential %q", name, ref)
			}
		}
	}

	bindings := make(map[string]struct{}, len(m.Spec.Bindings))
	for _, binding := range m.Spec.Bindings {
		buildTarget := strings.TrimSpace(binding.BuildTarget)
		if _, ok := buildTargets[buildTarget]; !ok {
			return fmt.Errorf("spec.bindings references unknown build target %q", buildTarget)
		}

		publishTarget := strings.TrimSpace(binding.PublishTarget)
		if _, ok := publishTargets[publishTarget]; !ok {
			return fmt.Errorf("spec.bindings references unknown publish target %q", publishTarget)
		}

		key := buildTarget + "->" + publishTarget
		if _, ok := bindings[key]; ok {
			return fmt.Errorf("spec.bindings contains duplicate pair %q", key)
		}
		bindings[key] = struct{}{}
	}

	return nil
}

// validateRequired ensures exactly one value source mechanism is configured.
func (v ValueSource) validateRequired() error {
	count := 0
	if strings.TrimSpace(v.Value) != "" {
		count++
	}
	if strings.TrimSpace(v.Env) != "" {
		count++
	}
	if strings.TrimSpace(v.File) != "" {
		count++
	}
	if count != 1 {
		return fmt.Errorf("exactly one of value, env, or file must be set")
	}

	return nil
}

// boolValue dereferences an optional boolean and falls back when it is unset.
func boolValue(input *bool, fallback bool) bool {
	if input == nil {
		return fallback
	}

	return *input
}

// runnerType resolves the effective runner type, falling back to the default
// build runner when the manifest leaves it blank.
func runnerType(input string) string {
	trimmed := strings.TrimSpace(input)
	if trimmed == "" {
		return build.DefaultRunnerType
	}

	return trimmed
}

// runnerTimeout resolves the effective runner timeout, falling back to the
// build default when the manifest omits or invalidates it.
func runnerTimeout(input int) int {
	if input <= 0 {
		return build.DefaultTimeoutSeconds
	}

	return input
}

// publishKind resolves the effective publish kind, falling back to the local
// filesystem publisher when the manifest leaves it blank.
func publishKind(input string) string {
	trimmed := strings.TrimSpace(input)
	if trimmed == "" {
		return publish.KindFilesystem
	}

	return trimmed
}
