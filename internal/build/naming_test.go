package build

import "testing"

func TestArtifactRepositoryNameNormalizesToASCIIHyphenSlug(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name          string
		repository    string
		repositoryURL string
		want          string
	}{
		{
			name:       "spaces and accents",
			repository: "Meu Repositório",
			want:       "meu-repositorio",
		},
		{
			name:       "special characters collapse",
			repository: "  Meu__Repositório!!!  ",
			want:       "meu-repositorio",
		},
		{
			name:          "fallback to repository url",
			repositoryURL: "https://github.com/indiegabo/Meu Repositório.git",
			want:          "meu-repositorio",
		},
	}

	for _, test := range tests {
		test := test
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()

			got := artifactRepositoryName(test.repository, test.repositoryURL)
			if got != test.want {
				t.Fatalf("artifactRepositoryName() = %q, want %q", got, test.want)
			}
		})
	}
}

func TestArtifactOutputRelativePathUsesNormalizedRepositorySlug(t *testing.T) {
	t.Parallel()

	plan := ExecutionPlan{
		RepositoryName: "Meu Repositório",
		GitTag:         "v1.2.3",
		TargetName:     "windows",
		OutputKind:     plainStringPointer("archive"),
	}

	got := artifactOutputRelativePath(plan)
	if got != "meu-repositorio.v1.2.3.windows.zip" {
		t.Fatalf("artifactOutputRelativePath() = %q", got)
	}
}
