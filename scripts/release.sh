#!/usr/bin/env bash
# release.sh — bump, commit, annotated-tag, push, and publish a GitHub release.
#
# Usage:
#   scripts/release.sh <version> <notes-file>
#   scripts/release.sh 0.3.1 /tmp/notes.md
#   scripts/release.sh 0.4.0 /tmp/notes.md --dry-run
#
# <version> is bare (no leading v). <notes-file> is markdown; it becomes both
# the annotated tag message and the GitHub release body.
#
# Assumes scripts/release-check.sh has passed — it re-runs only the cheap
# guards, not the gate suite.
#
# Everything before the first mutation is validated up front, so a bad
# invocation fails without touching the repo. If a later step fails, recovery
# instructions are printed.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
cd "$ROOT"

die() {
  echo "release: $*" >&2
  exit 1
}

VERSION="${1:-}"
NOTES="${2:-}"
DRY_RUN=false
[ "${3:-}" = "--dry-run" ] && DRY_RUN=true

[ -n "$VERSION" ] && [ -n "$NOTES" ] || die "usage: scripts/release.sh <version> <notes-file> [--dry-run]"

# --- validate (no mutations past this block) ---------------------------------

[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "version must be X.Y.Z (no leading v), got '$VERSION'"
TAG="v$VERSION"

[ -f "$NOTES" ] || die "notes file not found: $NOTES"
[ -s "$NOTES" ] || die "notes file is empty: $NOTES"

[ "$(git rev-parse --abbrev-ref HEAD)" = "main" ] || die "not on main"
# Tracked-only, matching release-check.sh: untracked files don't affect the release.
[ -z "$(git status --porcelain --untracked-files=no)" ] || die "uncommitted changes to tracked files"

git rev-parse -q --verify "refs/tags/$TAG" >/dev/null && die "tag $TAG already exists locally"
git ls-remote --exit-code --tags origin "$TAG" >/dev/null 2>&1 && die "tag $TAG already exists on origin"

CURRENT="$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)"
[ "$CURRENT" != "$VERSION" ] || die "Cargo.toml is already at $VERSION"

# Refuse to go backwards.
older="$(printf '%s\n%s\n' "$CURRENT" "$VERSION" | sort -V | head -1)"
[ "$older" = "$CURRENT" ] || die "$VERSION is older than current $CURRENT"

echo "releasing $CURRENT -> $VERSION"
echo "  notes: $NOTES ($(wc -l <"$NOTES" | tr -d ' ') lines)"

if $DRY_RUN; then
  echo
  echo "--dry-run: all preconditions pass; stopping before any mutation."
  exit 0
fi

# --- mutate ------------------------------------------------------------------

echo "  bumping Cargo.toml"
# Only the first `version = ` line — that's the package's own, not a dependency's.
sed -i "0,/^version = \"$CURRENT\"/s//version = \"$VERSION\"/" Cargo.toml
cargo check -q

changed="$(git diff --name-only)"
[ "$changed" = "$(printf 'Cargo.lock\nCargo.toml')" ] || {
  git checkout -- Cargo.toml Cargo.lock
  die "expected only Cargo.toml + Cargo.lock to change, got: $(echo "$changed" | tr '\n' ' ')"
}

echo "  committing"
git add Cargo.toml Cargo.lock
git commit -q -m "chore: release $TAG"

echo "  tagging $TAG (annotated)"
{
  echo "$TAG"
  echo
  cat "$NOTES"
} | git tag -a "$TAG" -F -

echo "  pushing"
if ! git push --quiet origin main --follow-tags; then
  cat >&2 <<EOF

push failed. The release commit and tag exist locally only. To undo:
  git tag -d $TAG
  git reset --hard HEAD~1
EOF
  exit 1
fi

git ls-remote --exit-code --tags origin "$TAG" >/dev/null \
  || die "push reported success but $TAG is not on origin"

echo "  publishing GitHub release"
if ! gh release create "$TAG" --verify-tag --title "$TAG" --notes-file "$NOTES"; then
  cat >&2 <<EOF

The commit and tag are pushed, but the GitHub release was not created.
Retry just that step with:
  gh release create $TAG --verify-tag --title "$TAG" --notes-file $NOTES
EOF
  exit 1
fi

echo
echo "released $TAG"
