package git

import (
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"strings"

	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
)

var (
	// ErrInvalidAuth reports unsupported or malformed Git credential material.
	ErrInvalidAuth = errors.New("invalid git auth configuration")
)

// AuthOptions describes the Git CLI configuration needed to authenticate one
// repository operation.
type AuthOptions struct {
	ExtraHeaders []string
}

// AppendGitArgs prefixes one Git command with the required authentication
// configuration flags.
func (o AuthOptions) AppendGitArgs(args ...string) []string {
	if len(o.ExtraHeaders) == 0 {
		return append([]string(nil), args...)
	}

	resolved := make([]string, 0, len(o.ExtraHeaders)*2+len(args))
	for _, header := range o.ExtraHeaders {
		trimmed := strings.TrimSpace(header)
		if trimmed == "" {
			continue
		}

		resolved = append(resolved, "-c", "http.extraHeader="+trimmed)
	}

	resolved = append(resolved, args...)
	return resolved
}

// AuthOptionsFromCredentials converts one stored credentials record into Git
// CLI authentication options.
func AuthOptionsFromCredentials(record credentials.Record) (AuthOptions, error) {
	switch strings.ToLower(strings.TrimSpace(record.Kind)) {
	case credentials.KindGitHTTPBasic:
		var config struct {
			Username string `json:"username"`
			Password string `json:"password"`
		}
		if err := decodeAuthConfig(record.ConfigJSON, &config); err != nil {
			return AuthOptions{}, err
		}

		username := strings.TrimSpace(config.Username)
		password := strings.TrimSpace(config.Password)
		if username == "" || password == "" {
			return AuthOptions{}, fmt.Errorf(
				"%w: git-http-basic requires username and password",
				ErrInvalidAuth,
			)
		}

		token := base64.StdEncoding.EncodeToString(
			[]byte(username + ":" + password),
		)
		return AuthOptions{
			ExtraHeaders: []string{"Authorization: Basic " + token},
		}, nil
	case credentials.KindGitHTTPBearer:
		var config struct {
			Token string `json:"token"`
		}
		if err := decodeAuthConfig(record.ConfigJSON, &config); err != nil {
			return AuthOptions{}, err
		}

		token := strings.TrimSpace(config.Token)
		if token == "" {
			return AuthOptions{}, fmt.Errorf(
				"%w: git-http-bearer requires token",
				ErrInvalidAuth,
			)
		}

		return AuthOptions{
			ExtraHeaders: []string{"Authorization: Bearer " + token},
		}, nil
	default:
		return AuthOptions{}, fmt.Errorf(
			"%w: unsupported credentials kind %q",
			ErrInvalidAuth,
			record.Kind,
		)
	}
}

// decodeAuthConfig unmarshals the stored credential JSON and wraps decode
// failures as Git authentication validation errors.
func decodeAuthConfig(raw string, target any) error {
	if err := json.Unmarshal([]byte(strings.TrimSpace(raw)), target); err != nil {
		return fmt.Errorf("%w: decode auth config: %v", ErrInvalidAuth, err)
	}

	return nil
}