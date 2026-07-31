#!/usr/bin/env bash
# release-check.sh — is the tree ready to cut a release, and what's in it?
#
# Usage:
#   scripts/release-check.sh
#
# Runs every mechanical precondition for a release and, if they all pass,
# prints the material needed to choose a version and write release notes:
# the current version, the last tag, and the commit range since it.
#
# Gate output (fmt/clippy/test) is captured and only shown on failure, so a
# passing run stays quiet.
#
# Exit codes: 0 ready, 1 not ready (reason printed to stderr).

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
cd "$ROOT"

LOG="$(mktemp -t release-check-XXXXXX.log)"
trap 'rm -f "$LOG"' EXIT

fail() {
  echo "not ready: $*" >&2
  exit 1
}

step() { printf '  %-34s' "$1"; }
ok() { echo "ok"; }

echo "checking release preconditions"

step "on main"
branch="$(git rev-parse --abbrev-ref HEAD)"
[ "$branch" = "main" ] || fail "on branch '$branch', expected main"
ok

step "no uncommitted changes"
# Tracked-only: untracked files aren't part of the release, and this repo keeps
# untracked working dirs around (.ai/, .claude/). They're reported below instead.
[ -z "$(git status --porcelain --untracked-files=no)" ] || {
  echo
  git status --short --untracked-files=no >&2
  fail "uncommitted changes to tracked files"
}
ok

untracked="$(git ls-files --others --exclude-standard)"
[ -z "$untracked" ] || {
  echo "  note: untracked files present (not in the release):"
  echo "$untracked" | sed 's/^/    /'
}

step "in sync with origin"
git fetch --quiet origin main
behind="$(git rev-list --count HEAD..origin/main)"
[ "$behind" -eq 0 ] || fail "$behind commit(s) behind origin/main — pull first"
ok

step "gh authenticated"
gh auth status >>"$LOG" 2>&1 || fail "gh not authenticated (see: gh auth login)"
ok

step "cargo fmt"
cargo fmt --all --check >>"$LOG" 2>&1 || {
  echo
  cat "$LOG" >&2
  fail "formatting differs (run: cargo fmt --all)"
}
ok

step "cargo clippy"
cargo clippy --workspace --all-targets -- -D warnings >>"$LOG" 2>&1 || {
  echo
  tail -40 "$LOG" >&2
  fail "clippy warnings"
}
ok

step "cargo test"
cargo test --workspace >>"$LOG" 2>&1 || {
  echo
  tail -40 "$LOG" >&2
  fail "tests failing"
}
ok

version="$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)"
last_tag="$(git describe --tags --abbrev=0 2>/dev/null || true)"
[ -n "$last_tag" ] || fail "no tags in this repo — cut the first one by hand"

range="$last_tag..HEAD"
count="$(git rev-list --count "$range")"
[ "$count" -gt 0 ] || fail "no commits since $last_tag"

tests="$(grep -c '^test result: ok' "$LOG" || true)"
passed="$(awk '/^test result: ok/ {n += $4} END {print n+0}' "$LOG")"

cat <<EOF

ready to release.

  current version   $version
  last tag          $last_tag
  commits since     $count
  tests passing     $passed (across $tests binaries)

commits in $range:

EOF

git log --oneline --no-decorate "$range"

cat <<EOF

files touched, by area:

EOF

git diff --stat "$range" -- \
  | tail -1
git diff --name-only "$range" \
  | awk -F/ '{ print ($1 == "src" || $1 == "docs") ? $1 "/" $2 : $1 }' \
  | sort | uniq -c | sort -rn

cat <<EOF

next: pick a version, write the notes, then
  scripts/release.sh <version> <notes-file>
EOF
