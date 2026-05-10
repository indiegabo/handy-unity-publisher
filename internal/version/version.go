// Package version exposes build metadata for binaries and HTTP responses.
package version

// buildVersion holds the compile-time version string injected by the build.
var buildVersion = "dev"

// String returns the build version injected at compile time.
func String() string {
	return buildVersion
}