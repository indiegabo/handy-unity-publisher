package build

import "testing"

func TestGameCIImageResolverUsesResolvedUnityVersionAndPlatform(t *testing.T) {
	t.Parallel()

	resolver := newGameCIImageResolver()

	image, err := resolver.Resolve(Target{
		Platform:   "webgl",
		RunnerType: DefaultRunnerType,
	}, "2022.3.14f1")
	if err != nil {
		t.Fatalf("resolve image: %v", err)
	}

	if image != "unityci/editor:ubuntu-2022.3.14f1-webgl-3" {
		t.Fatalf("expected resolved image unityci/editor:ubuntu-2022.3.14f1-webgl-3, got %q", image)
	}
}

func TestGameCIImageResolverUsesImageOverride(t *testing.T) {
	t.Parallel()

	override := "ghcr.io/example/custom-unity:2022.3.14f1"
	resolver := newGameCIImageResolver()

	image, err := resolver.Resolve(Target{
		Platform:      "webgl",
		RunnerType:    DefaultRunnerType,
		ImageOverride: &override,
	}, "2022.3.14f1")
	if err != nil {
		t.Fatalf("resolve image: %v", err)
	}

	if image != override {
		t.Fatalf("expected image override %q, got %q", override, image)
	}
}

func TestResolveTargetUnityVersionUsesOverrideWhenPresent(t *testing.T) {
	t.Parallel()

	override := "2021.3.18f1"
	version := resolveTargetUnityVersion(Target{UnityVersionOverride: &override}, "2022.3.14f1")
	if version != override {
		t.Fatalf("expected target unity version override %q, got %q", override, version)
	}
}

func TestGameCIImageResolverRejectsUnsupportedPlatform(t *testing.T) {
	t.Parallel()

	resolver := newGameCIImageResolver()

	_, err := resolver.Resolve(Target{
		Platform:   "playstation",
		RunnerType: DefaultRunnerType,
	}, "2022.3.14f1")
	if err == nil {
		t.Fatal("expected unsupported platform error")
	}
}