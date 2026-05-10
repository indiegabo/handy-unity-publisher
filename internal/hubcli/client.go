package hubcli

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"strings"
	"time"

	"github.com/indiegabo/handy-unity-bulder/internal/automation"
	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
	"github.com/indiegabo/handy-unity-bulder/internal/pipelines"
	"github.com/indiegabo/handy-unity-bulder/internal/publish"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	"github.com/indiegabo/handy-unity-bulder/internal/trigger"
)

// defaultBaseURL is the fallback runtime address used when HUB_BASE_URL is not
// configured.
const defaultBaseURL = "http://127.0.0.1:8080"

// client is the shared HTTP transport used by hub subcommands.
type client struct {
	baseURL    string
	httpClient *http.Client
}

// apiErrorResponse matches the normalized JSON error envelope returned by the
// server API.
type apiErrorResponse struct {
	Error string `json:"error"`
}

// repositoryCreateRequest is the JSON payload used when hub creates one
// repository over HTTP.
type repositoryCreateRequest struct {
	Name                   string `json:"name"`
	RepoURL                string `json:"repo_url"`
	CredentialsID          *int64 `json:"credentials_id,omitempty"`
	DefaultBranch          string `json:"default_branch"`
	PollingIntervalSeconds int    `json:"polling_interval_seconds"`
	Enabled                *bool  `json:"enabled,omitempty"`
}

// credentialsCreateRequest is the JSON payload used when hub creates one
// credentials record over HTTP.
type credentialsCreateRequest struct {
	Name       string `json:"name"`
	Kind       string `json:"kind"`
	ConfigJSON string `json:"config_json"`
}

// buildTargetCreateRequest is the JSON payload used when hub creates one
// build target over HTTP.
type buildTargetCreateRequest struct {
	RepositoryID         int64  `json:"repository_id"`
	Name                 string `json:"name"`
	Platform             string `json:"platform"`
	RunnerType           string `json:"runner_type,omitempty"`
	BuildMethod          string `json:"build_method,omitempty"`
	OutputKind           string `json:"output_kind,omitempty"`
	OutputPathTemplate   string `json:"output_path_template,omitempty"`
	UnityVersionOverride string `json:"unity_version_override,omitempty"`
	ImageOverride        string `json:"image_override,omitempty"`
	TimeoutSeconds       int    `json:"timeout_seconds,omitempty"`
	Enabled              *bool  `json:"enabled,omitempty"`
	ConfigJSON           string `json:"config_json"`
}

// triggerRuleCreateRequest is the JSON payload used when hub creates one
// trigger rule over HTTP.
type triggerRuleCreateRequest struct {
	RepositoryID int64  `json:"repository_id"`
	Name         string `json:"name"`
	Source       string `json:"source"`
	Enabled      *bool  `json:"enabled,omitempty"`
	ConfigJSON   string `json:"config_json"`
}

// publishTargetCreateRequest is the JSON payload used when hub creates one
// publish target over HTTP.
type publishTargetCreateRequest struct {
	RepositoryID  int64  `json:"repository_id"`
	Name          string `json:"name"`
	Kind          string `json:"kind"`
	CredentialsID *int64 `json:"credentials_id,omitempty"`
	Enabled       *bool  `json:"enabled,omitempty"`
	ConfigJSON    string `json:"config_json"`
}

// buildPublishBindingCreateRequest is the JSON payload used when hub creates
// one build-to-publish binding over HTTP.
type buildPublishBindingCreateRequest struct {
	BuildTargetID   int64  `json:"build_target_id"`
	PublishTargetID int64  `json:"publish_target_id"`
	Enabled         *bool  `json:"enabled,omitempty"`
	OptionsJSON     string `json:"options_json"`
}

// manualReleaseDispatchClientRequest is the JSON payload used when hub requests
// one manual release dispatch over HTTP.
type manualReleaseDispatchClientRequest struct {
	RepositoryID int64  `json:"repository_id"`
	GitTag       string `json:"git_tag"`
	GitCommit    string `json:"git_commit,omitempty"`
	Rebuild      bool   `json:"rebuild,omitempty"`
}

// serviceUnavailableError reports that hub could not reach the target runtime.
type serviceUnavailableError struct {
	baseURL string
	cause   error
}

// Error formats the runtime availability failure for operator-facing output.
func (e *serviceUnavailableError) Error() string {
	return fmt.Sprintf(
		"handy-unity-bulder is not reachable at %s: %v. Start it with `docker compose up -d` from the project root and retry.",
		e.baseURL,
		e.cause,
	)
}

// newClient builds the default hub HTTP client from environment settings.
func newClient() *client {
	baseURL := strings.TrimSpace(os.Getenv("HUB_BASE_URL"))
	if baseURL == "" {
		baseURL = defaultBaseURL
	}

	return &client{
		baseURL: strings.TrimRight(baseURL, "/"),
		httpClient: &http.Client{
			Timeout: 10 * time.Second,
		},
	}
}

// ensureAvailable probes the runtime health endpoint before a subcommand makes
// heavier API requests.
func (c *client) ensureAvailable(ctx context.Context) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, c.baseURL+"/healthz", nil)
	if err != nil {
		return fmt.Errorf("build health request: %w", err)
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return &serviceUnavailableError{baseURL: c.baseURL, cause: err}
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return &serviceUnavailableError{
			baseURL: c.baseURL,
			cause:   fmt.Errorf("health endpoint returned status %d", resp.StatusCode),
		}
	}

	return nil
}

// listRepositories loads all repositories from the HTTP API.
func (c *client) listRepositories(ctx context.Context) ([]repository.Record, error) {
	var records []repository.Record
	if err := c.getJSON(ctx, "/api/v1/repositories", &records); err != nil {
		return nil, err
	}

	return records, nil
}

// getRepository loads one repository by identifier from the HTTP API.
func (c *client) getRepository(ctx context.Context, id int64) (repository.Record, error) {
	var record repository.Record
	if err := c.getJSON(ctx, fmt.Sprintf("/api/v1/repositories/%d", id), &record); err != nil {
		return repository.Record{}, err
	}

	return record, nil
}

// createRepository posts one repository create request to the HTTP API.
func (c *client) createRepository(ctx context.Context, request repositoryCreateRequest) (repository.Record, error) {
	var record repository.Record
	if err := c.postJSON(ctx, "/api/v1/repositories", request, &record); err != nil {
		return repository.Record{}, err
	}

	return record, nil
}

// dispatchManualRelease posts one manual release request to the HTTP API.
func (c *client) dispatchManualRelease(
	ctx context.Context,
	request manualReleaseDispatchClientRequest,
) (release.Record, error) {
	var record release.Record
	if err := c.postJSON(ctx, "/api/v1/releases/dispatch/manual", request, &record); err != nil {
		return release.Record{}, err
	}

	return record, nil
}

// listCredentials loads all credentials records from the HTTP API.
func (c *client) listCredentials(ctx context.Context) ([]credentials.Record, error) {
	var records []credentials.Record
	if err := c.getJSON(ctx, "/api/v1/credentials", &records); err != nil {
		return nil, err
	}

	return records, nil
}

// getCredentials loads one credentials record by identifier from the HTTP API.
func (c *client) getCredentials(ctx context.Context, id int64) (credentials.Record, error) {
	var record credentials.Record
	if err := c.getJSON(ctx, fmt.Sprintf("/api/v1/credentials/%d", id), &record); err != nil {
		return credentials.Record{}, err
	}

	return record, nil
}

// createCredentials posts one credentials create request to the HTTP API.
func (c *client) createCredentials(ctx context.Context, request credentialsCreateRequest) (credentials.Record, error) {
	var record credentials.Record
	if err := c.postJSON(ctx, "/api/v1/credentials", request, &record); err != nil {
		return credentials.Record{}, err
	}

	return record, nil
}

// listBuildTargets loads build targets for one repository from the HTTP API.
func (c *client) listBuildTargets(ctx context.Context, repositoryID int64) ([]build.Target, error) {
	query := url.Values{}
	query.Set("repository_id", fmt.Sprintf("%d", repositoryID))

	var targets []build.Target
	if err := c.getJSON(ctx, buildPathWithQuery("/api/v1/build-targets", query), &targets); err != nil {
		return nil, err
	}

	return targets, nil
}

// getBuildTarget loads one build target by identifier from the HTTP API.
func (c *client) getBuildTarget(ctx context.Context, id int64) (build.Target, error) {
	var target build.Target
	if err := c.getJSON(ctx, fmt.Sprintf("/api/v1/build-targets/%d", id), &target); err != nil {
		return build.Target{}, err
	}

	return target, nil
}

// createBuildTarget posts one build target create request to the HTTP API.
func (c *client) createBuildTarget(ctx context.Context, request buildTargetCreateRequest) (build.Target, error) {
	var target build.Target
	if err := c.postJSON(ctx, "/api/v1/build-targets", request, &target); err != nil {
		return build.Target{}, err
	}

	return target, nil
}

// listTriggerRules loads trigger rules for one repository from the HTTP API.
func (c *client) listTriggerRules(ctx context.Context, repositoryID int64) ([]trigger.Rule, error) {
	query := url.Values{}
	query.Set("repository_id", fmt.Sprintf("%d", repositoryID))

	var rules []trigger.Rule
	if err := c.getJSON(ctx, buildPathWithQuery("/api/v1/trigger-rules", query), &rules); err != nil {
		return nil, err
	}

	return rules, nil
}

// getTriggerRule loads one trigger rule by identifier from the HTTP API.
func (c *client) getTriggerRule(ctx context.Context, id int64) (trigger.Rule, error) {
	var rule trigger.Rule
	if err := c.getJSON(ctx, fmt.Sprintf("/api/v1/trigger-rules/%d", id), &rule); err != nil {
		return trigger.Rule{}, err
	}

	return rule, nil
}

// createTriggerRule posts one trigger rule create request to the HTTP API.
func (c *client) createTriggerRule(ctx context.Context, request triggerRuleCreateRequest) (trigger.Rule, error) {
	var rule trigger.Rule
	if err := c.postJSON(ctx, "/api/v1/trigger-rules", request, &rule); err != nil {
		return trigger.Rule{}, err
	}

	return rule, nil
}

// listPublishTargets loads publish targets for one repository from the HTTP
// API.
func (c *client) listPublishTargets(ctx context.Context, repositoryID int64) ([]publish.Target, error) {
	query := url.Values{}
	query.Set("repository_id", fmt.Sprintf("%d", repositoryID))

	var targets []publish.Target
	if err := c.getJSON(ctx, buildPathWithQuery("/api/v1/publish-targets", query), &targets); err != nil {
		return nil, err
	}

	return targets, nil
}

// getPublishTarget loads one publish target by identifier from the HTTP API.
func (c *client) getPublishTarget(ctx context.Context, id int64) (publish.Target, error) {
	var target publish.Target
	if err := c.getJSON(ctx, fmt.Sprintf("/api/v1/publish-targets/%d", id), &target); err != nil {
		return publish.Target{}, err
	}

	return target, nil
}

// createPublishTarget posts one publish target create request to the HTTP API.
func (c *client) createPublishTarget(ctx context.Context, request publishTargetCreateRequest) (publish.Target, error) {
	var target publish.Target
	if err := c.postJSON(ctx, "/api/v1/publish-targets", request, &target); err != nil {
		return publish.Target{}, err
	}

	return target, nil
}

// listBuildPublishBindings loads bindings for one build target from the HTTP
// API.
func (c *client) listBuildPublishBindings(ctx context.Context, buildTargetID int64) ([]publish.Binding, error) {
	query := url.Values{}
	query.Set("build_target_id", fmt.Sprintf("%d", buildTargetID))

	var bindings []publish.Binding
	if err := c.getJSON(ctx, buildPathWithQuery("/api/v1/build-publish-bindings", query), &bindings); err != nil {
		return nil, err
	}

	return bindings, nil
}

// getBuildPublishBinding loads one binding by identifier from the HTTP API.
func (c *client) getBuildPublishBinding(ctx context.Context, id int64) (publish.Binding, error) {
	var binding publish.Binding
	if err := c.getJSON(ctx, fmt.Sprintf("/api/v1/build-publish-bindings/%d", id), &binding); err != nil {
		return publish.Binding{}, err
	}

	return binding, nil
}

// createBuildPublishBinding posts one binding create request to the HTTP API.
func (c *client) createBuildPublishBinding(
	ctx context.Context,
	request buildPublishBindingCreateRequest,
) (publish.Binding, error) {
	var binding publish.Binding
	if err := c.postJSON(ctx, "/api/v1/build-publish-bindings", request, &binding); err != nil {
		return publish.Binding{}, err
	}

	return binding, nil
}

// buildPathWithQuery appends an encoded query string to one relative API path.
func buildPathWithQuery(path string, query url.Values) string {
	if len(query) == 0 {
		return path
	}

	return path + "?" + query.Encode()
}

// exportDatabase downloads one database snapshot into the provided writer.
func (c *client) exportDatabase(ctx context.Context, writer io.Writer) error {
	if writer == nil {
		return fmt.Errorf("database export writer must not be nil")
	}

	req, err := http.NewRequestWithContext(
		ctx,
		http.MethodGet,
		c.baseURL+"/api/v1/runtime/database/export",
		nil,
	)
	if err != nil {
		return fmt.Errorf("build database export request: %w", err)
	}

	return c.stream(req, writer, databaseTransferTimeout)
}

// importDatabase uploads one database snapshot to the runtime import endpoint.
func (c *client) importDatabase(ctx context.Context, reader io.Reader) error {
	if reader == nil {
		return fmt.Errorf("database import reader must not be nil")
	}

	req, err := http.NewRequestWithContext(
		ctx,
		http.MethodPost,
		c.baseURL+"/api/v1/runtime/database/import",
		reader,
	)
	if err != nil {
		return fmt.Errorf("build database import request: %w", err)
	}
	req.Header.Set("Content-Type", "application/octet-stream")

	return c.doWithTimeout(req, nil, databaseTransferTimeout)
}

// runtimePipelines returns the last declarative pipeline synchronization report.
func (c *client) runtimePipelines(ctx context.Context) (pipelines.ApplyReport, error) {
	req, err := http.NewRequestWithContext(
		ctx,
		http.MethodGet,
		c.baseURL+"/api/v1/runtime/pipelines",
		nil,
	)
	if err != nil {
		return pipelines.ApplyReport{}, fmt.Errorf("build runtime pipelines request: %w", err)
	}

	var report pipelines.ApplyReport
	if err := c.doWithTimeout(req, &report, c.httpClient.Timeout); err != nil {
		return pipelines.ApplyReport{}, err
	}

	return report, nil
}

// runtimeAutomation returns the current polling and release-backlog snapshot
// from the runtime automation coordinator.
func (c *client) runtimeAutomation(ctx context.Context) (automation.RuntimeReport, error) {
	req, err := http.NewRequestWithContext(
		ctx,
		http.MethodGet,
		c.baseURL+"/api/v1/runtime/automation",
		nil,
	)
	if err != nil {
		return automation.RuntimeReport{}, fmt.Errorf(
			"build runtime automation request: %w",
			err,
		)
	}

	var report automation.RuntimeReport
	if err := c.doWithTimeout(req, &report, c.httpClient.Timeout); err != nil {
		return automation.RuntimeReport{}, err
	}

	return report, nil
}

// getJSON executes one GET request and decodes the JSON response body.
func (c *client) getJSON(ctx context.Context, path string, dst any) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, c.baseURL+path, nil)
	if err != nil {
		return fmt.Errorf("build GET request %s: %w", path, err)
	}

	return c.do(req, dst)
}

// postJSON executes one POST request with a JSON body and decodes the JSON
// response body.
func (c *client) postJSON(ctx context.Context, path string, payload any, dst any) error {
	encoded, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("encode request for %s: %w", path, err)
	}

	req, err := http.NewRequestWithContext(
		ctx,
		http.MethodPost,
		c.baseURL+path,
		bytes.NewReader(encoded),
	)
	if err != nil {
		return fmt.Errorf("build POST request %s: %w", path, err)
	}
	req.Header.Set("Content-Type", "application/json")

	return c.do(req, dst)
}

// do executes one request using the client's default timeout budget.
func (c *client) do(req *http.Request, dst any) error {
	return c.doWithTimeout(req, dst, c.httpClient.Timeout)
}

// doWithTimeout executes one JSON-oriented request with an explicit timeout override.
func (c *client) doWithTimeout(req *http.Request, dst any, timeout time.Duration) error {
	client := *c.httpClient
	client.Timeout = timeout

	resp, err := client.Do(req)
	if err != nil {
		if errors.Is(err, context.DeadlineExceeded) {
			return fmt.Errorf("request timeout for %s %s", req.Method, req.URL.Path)
		}

		return err
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return fmt.Errorf(
			"read response body for %s %s: %w",
			req.Method,
			req.URL.Path,
			err,
		)
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		var apiErr apiErrorResponse
		if err := json.Unmarshal(body, &apiErr); err == nil &&
			strings.TrimSpace(apiErr.Error) != "" {
			return errors.New(apiErr.Error)
		}

		if len(body) == 0 {
			return fmt.Errorf("request failed with status %d", resp.StatusCode)
		}

		return fmt.Errorf(
			"request failed with status %d: %s",
			resp.StatusCode,
			strings.TrimSpace(string(body)),
		)
	}

	if dst == nil || len(body) == 0 {
		return nil
	}

	if err := json.Unmarshal(body, dst); err != nil {
		return fmt.Errorf(
			"decode response for %s %s: %w",
			req.Method,
			req.URL.Path,
			err,
		)
	}

	return nil
}

// stream executes one request and copies the raw response body into writer.
func (c *client) stream(req *http.Request, writer io.Writer, timeout time.Duration) error {
	client := *c.httpClient
	client.Timeout = timeout

	resp, err := client.Do(req)
	if err != nil {
		if errors.Is(err, context.DeadlineExceeded) {
			return fmt.Errorf("request timeout for %s %s", req.Method, req.URL.Path)
		}

		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		body, readErr := io.ReadAll(resp.Body)
		if readErr != nil {
			return fmt.Errorf(
				"read error response body for %s %s: %w",
				req.Method,
				req.URL.Path,
				readErr,
			)
		}

		var apiErr apiErrorResponse
		if err := json.Unmarshal(body, &apiErr); err == nil &&
			strings.TrimSpace(apiErr.Error) != "" {
			return errors.New(apiErr.Error)
		}

		if len(body) == 0 {
			return fmt.Errorf("request failed with status %d", resp.StatusCode)
		}

		return fmt.Errorf(
			"request failed with status %d: %s",
			resp.StatusCode,
			strings.TrimSpace(string(body)),
		)
	}

	if _, err := io.Copy(writer, resp.Body); err != nil {
		return fmt.Errorf(
			"copy response body for %s %s: %w",
			req.Method,
			req.URL.Path,
			err,
		)
	}

	return nil
}
