#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd -- "$script_dir/.." && pwd)

# Keep isolated Cargo outputs under the ignored tmp tree instead of the repo root.
: "${CARGO_TARGET_DIR:=$repo_root/tmp/cargo-targets/runtime-smoke}"

if [[ -n "${CARGO_BIN:-}" ]]; then
	cargo_bin="$CARGO_BIN"
elif command -v cargo >/dev/null 2>&1; then
	cargo_bin=$(command -v cargo)
elif command -v cargo.exe >/dev/null 2>&1; then
	cargo_bin=$(command -v cargo.exe)
elif [[ -x "${HOME:-}/.cargo/bin/cargo" ]]; then
	cargo_bin="${HOME}/.cargo/bin/cargo"
elif [[ -x "${HOME:-}/.cargo/bin/cargo.exe" ]]; then
	cargo_bin="${HOME}/.cargo/bin/cargo.exe"
else
	echo "runtime smoke could not resolve cargo; set CARGO_BIN explicitly" >&2
	exit 127
fi

cd "$repo_root"
mkdir -p "$CARGO_TARGET_DIR"
export CARGO_TARGET_DIR

echo "Running runtime interrupted-recovery smoke tests with CARGO_TARGET_DIR=$CARGO_TARGET_DIR"
exec "$cargo_bin" test -p runtime-bin --test interrupted_cleanup_e2e -- --nocapture