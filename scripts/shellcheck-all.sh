#!/usr/bin/env bash
# Lint every shell script tracked in this repo (scripts/shellcheck-all.sh).
#
# Usage:
#   scripts/shellcheck-all.sh
#
# One entry point so CI and scripts/release-check.sh run the same command over
# the same file set. The set comes from `git ls-files '*.sh'`, not a glob, so a
# script added in a new directory is covered without editing anything here.
#
# SEVERITY is `style`, which is everything shellcheck has to say. It is NOT the
# severity the whole tree passes at today — when this gate landed, 11 of 32
# scripts had findings and 21 were already clean, and the three loudest are
# false positives about regexes rather than defects. Weakening the severity to
# `error` would not have helped: nothing is clean at `error` either. So the
# exempt files are named below and everything else is held to the full bar,
# rather than the bar being lowered for all 32 to accommodate 11.
#
# The list only ever shrinks. A file on it that shellcheck now finds clean fails
# this script, because an exemption nobody is forced to remove is one nobody
# removes. Cleaning those files is its own slice; deleting a line from the list
# is how it lands.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
cd "$ROOT" || exit 1

SEVERITY="${SHELLCHECK_SEVERITY:-style}"

# Not clean at -S style as of 2026-09-17. Remove a line when you fix the file.
NOT_YET_CLEAN=(
  "install.sh"
  "scripts/check-sensitive-metadata.sh"
  "scripts/experiment-jwt-key-revocation.sh"
  "scripts/experiment-managed-hook-relationships.sh"
  "scripts/experiment-managed-nested-object-query.sh"
  "scripts/release-check.sh"
  "scripts/rhino-script-tester/run-atm-probes.sh"
  "scripts/rhino-script-tester/run-oauth2-probes.sh"
  "scripts/rhino-script-tester/run-probes.sh"
  "scripts/verify-pattern1-cookie.sh"
  "scripts/verify-pattern2-userpass.sh"
)

command -v shellcheck >/dev/null 2>&1 || {
  echo "shellcheck-all: shellcheck is not on PATH" >&2
  exit 1
}

in_list() {
  local needle="$1" item
  shift
  for item in "$@"; do [ "$item" = "$needle" ] && return 0; done
  return 1
}

mapfile -t all < <(git ls-files '*.sh' | sort)
[ "${#all[@]}" -gt 0 ] || {
  echo "shellcheck-all: git ls-files matched no shell scripts" >&2
  exit 1
}

checked=()
skipped=()
for f in "${all[@]}"; do
  if in_list "$f" "${NOT_YET_CLEAN[@]}"; then
    skipped+=("$f")
  else
    checked+=("$f")
  fi
done

status=0
if [ "${#checked[@]}" -gt 0 ]; then
  shellcheck -S "$SEVERITY" "${checked[@]}" || status=1
fi

for f in "${NOT_YET_CLEAN[@]}"; do
  if ! in_list "$f" "${all[@]}"; then
    echo "shellcheck-all: $f is exempt but is not a tracked shell script; remove it from NOT_YET_CLEAN" >&2
    status=1
  elif shellcheck -S "$SEVERITY" "$f" >/dev/null 2>&1; then
    echo "shellcheck-all: $f is exempt but is now clean at -S $SEVERITY; remove it from NOT_YET_CLEAN" >&2
    status=1
  fi
done

printf 'shellcheck -S %s: %d checked, %d exempt (see NOT_YET_CLEAN in %s)\n' \
  "$SEVERITY" "${#checked[@]}" "${#skipped[@]}" "scripts/shellcheck-all.sh"

exit "$status"
