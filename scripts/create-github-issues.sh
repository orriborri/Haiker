#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

printf '%s\n' \
  'scripts/create-github-issues.sh now delegates to the idempotent sync.' \
  'Dry-run is the default; pass --apply to write to GitHub.'

exec "$SCRIPT_DIR/sync-github-issues.sh" "$@"
