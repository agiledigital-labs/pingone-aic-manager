#!/usr/bin/env bash
# Run gitleaks at the version CI pins (scripts/gitleaks.sh [gitleaks args…]).
#
# Usage:
#   scripts/gitleaks.sh git . --redact --no-banner
#   scripts/gitleaks.sh --path     # print the binary's path, fetching it if needed
#
# The version is read from .github/workflows/ci.yml, so the git hooks,
# scripts/release-check.sh and CI cannot run different rulesets. A gitleaks on
# PATH is used only at that exact version; otherwise the release binary is
# fetched once into ~/.cache/pingone-aic-manager/. .gitleaks.toml is picked up
# from the repo root.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version="$(grep -oP '^\s+GITLEAKS_VERSION:\s*\K\S+' "$root/.github/workflows/ci.yml" | head -1)"
if [ -z "$version" ]; then
  echo "gitleaks.sh: could not read GITLEAKS_VERSION from ci.yml" >&2
  exit 2
fi

cache="${XDG_CACHE_HOME:-$HOME/.cache}/pingone-aic-manager"
bin="$cache/gitleaks-$version"
if command -v gitleaks >/dev/null 2>&1 && [ "$(gitleaks version 2>/dev/null)" = "$version" ]; then
  bin="$(command -v gitleaks)"
elif [ ! -x "$bin" ]; then
  echo "gitleaks.sh: fetching gitleaks $version" >&2
  mkdir -p "$cache"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  if ! curl -sSfL "https://github.com/gitleaks/gitleaks/releases/download/v${version}/gitleaks_${version}_linux_x64.tar.gz" |
    tar -xz -C "$tmp" gitleaks; then
    echo "gitleaks.sh: could not fetch gitleaks $version. Install it on PATH at that exact version." >&2
    exit 2
  fi
  mv "$tmp/gitleaks" "$bin"
fi

if [ "${1:-}" = --path ]; then
  printf '%s\n' "$bin"
  exit 0
fi
cd "$root"
exec "$bin" "$@"
