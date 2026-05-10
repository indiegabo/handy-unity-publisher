package git

import (
	"context"
	"errors"
	"reflect"
	"testing"
)

func TestRemoteTagSourceListsTagsInGitOrder(t *testing.T) {
	t.Parallel()

	runner := &commandRunnerStub{
		output: []byte(
			"111111\trefs/tags/v1.0.0\n222222\trefs/tags/v1.1.0\n",
		),
	}
	source := newRemoteTagSourceWithRunner(runner)

	tags, err := source.ListTags(
		context.Background(),
		"https://example.com/org/repo.git",
		AuthOptions{},
	)
	if err != nil {
		t.Fatalf("list tags: %v", err)
	}

	if runner.name != "git" {
		t.Fatalf("expected git command, got %q", runner.name)
	}

	expectedArgs := []string{
		"ls-remote",
		"--refs",
		"--tags",
		"--sort=version:refname",
		"https://example.com/org/repo.git",
	}
	if !reflect.DeepEqual(runner.args, expectedArgs) {
		t.Fatalf("expected args %#v, got %#v", expectedArgs, runner.args)
	}

	expectedTags := []Tag{
		{Name: "v1.0.0", Commit: "111111"},
		{Name: "v1.1.0", Commit: "222222"},
	}
	if !reflect.DeepEqual(tags, expectedTags) {
		t.Fatalf("expected tags %#v, got %#v", expectedTags, tags)
	}
}

func TestRemoteTagSourceReturnsEmptySliceWhenRepositoryHasNoTags(t *testing.T) {
	t.Parallel()

	source := newRemoteTagSourceWithRunner(&commandRunnerStub{})

	tags, err := source.ListTags(context.Background(), "https://example.com/org/repo.git", AuthOptions{})
	if err != nil {
		t.Fatalf("list tags: %v", err)
	}

	if len(tags) != 0 {
		t.Fatalf("expected no tags, got %#v", tags)
	}
}

func TestRemoteTagSourceRejectsEmptyRepositoryURL(t *testing.T) {
	t.Parallel()

	source := newRemoteTagSourceWithRunner(&commandRunnerStub{})

	_, err := source.ListTags(context.Background(), " ", AuthOptions{})
	if !errors.Is(err, ErrInvalidRepositoryURL) {
		t.Fatalf("expected invalid repository url error, got %v", err)
	}
}

func TestRemoteTagSourceReturnsGitCommandErrors(t *testing.T) {
	t.Parallel()

	source := newRemoteTagSourceWithRunner(&commandRunnerStub{err: errors.New("git offline")})

	_, err := source.ListTags(context.Background(), "https://example.com/org/repo.git", AuthOptions{})
	if err == nil {
		t.Fatal("expected git command error")
	}
}

func TestRemoteTagSourcePrefixesGitAuthConfiguration(t *testing.T) {
	t.Parallel()

	runner := &commandRunnerStub{}
	source := newRemoteTagSourceWithRunner(runner)

	_, err := source.ListTags(
		context.Background(),
		"https://example.com/org/repo.git",
		AuthOptions{ExtraHeaders: []string{"Authorization: Bearer secret"}},
	)
	if err != nil {
		t.Fatalf("list tags: %v", err)
	}

	expectedArgs := []string{
		"-c",
		"http.extraHeader=Authorization: Bearer secret",
		"ls-remote",
		"--refs",
		"--tags",
		"--sort=version:refname",
		"https://example.com/org/repo.git",
	}
	if !reflect.DeepEqual(runner.args, expectedArgs) {
		t.Fatalf("expected args %#v, got %#v", expectedArgs, runner.args)
	}
}

type commandRunnerStub struct {
	output []byte
	err    error
	name   string
	args   []string
}

func (s *commandRunnerStub) Run(
	_ context.Context,
	name string,
	args ...string,
) ([]byte, error) {
	s.name = name
	s.args = append([]string(nil), args...)
	if s.err != nil {
		return nil, s.err
	}

	return append([]byte(nil), s.output...), nil
}