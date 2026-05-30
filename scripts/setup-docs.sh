#!/usr/bin/env bash
# POSIX shell script to set up docs virtualenv and install dependencies.
# Usage: ./scripts/setup-docs.sh

set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.."; pwd)"
venv_path="$repo_root/tmp/docs-venv"
echo "Creating virtual environment at $venv_path"
python3 -m venv "$venv_path"
"$venv_path/bin/python" -m pip install --upgrade pip
"$venv_path/bin/python" -m pip install -r "$repo_root/requirements.txt"
echo "Done. To preview docs run:"
echo "  $venv_path/bin/python -m mkdocs serve"
