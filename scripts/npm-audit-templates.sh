#!/usr/bin/env bash
# Audit the npm dependencies the shipped script-workspace templates install
# (scripts/npm-audit-templates.sh).
#
# Usage:
#   scripts/npm-audit-templates.sh
#
# `aic` copies these manifests into users' workspaces, and they carry no
# lockfile, so the tree a user gets is whatever the ranges resolve to today.
# Each one is resolved into a throwaway directory (lockfile only: no install, no
# scripts) and that lockfile is audited, failing on any advisory. Run by CI and
# by scripts/release-check.sh; the root package-lock.json is audited separately.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifests=(
  src/scripts/templates/package.json
  src/scripts/templates/typescript/package.json
  src/scripts/templates/tools/package.json
)

for manifest in "${manifests[@]}"; do
  dir="$(mktemp -d)"
  trap 'rm -rf "$dir"' EXIT
  cp "$root/$manifest" "$dir/package.json"
  echo "npm audit: $manifest"
  (
    cd "$dir"
    npm install --package-lock-only --ignore-scripts --no-audit --no-fund >/dev/null
    npm audit --audit-level=low
  )
  rm -rf "$dir"
done
