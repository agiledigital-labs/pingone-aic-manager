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
# THE GATES HERE ARE THE GATES CI RUNS. Not a subset, and not a paraphrase —
# the same commands, in the same order, with the same flags. A release cut from
# a tree this script called ready must not then fail CI, because by then the tag
# is pushed and the artifacts are published.
#
# That parity used to be maintained by hand and rotted: for several releases
# this script ran `cargo clippy`/`cargo test` without `--features logs-store`,
# so it could report "ready to release" with the DuckDB log store broken, and
# it ran neither the sensitive-metadata scanner nor gitleaks nor the TypeScript
# type gates at all. `ci_parity` below now fails when .github/workflows/ci.yml
# gains or loses a step this script does not account for, so the next divergence
# is a loud failure here rather than a red build after the release is public.
#
# Gate output is captured and only shown on failure, so a passing run stays
# quiet.
#
# Exit codes: 0 ready, 1 not ready (reason printed to stderr).

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
cd "$ROOT"

CI_YML=".github/workflows/ci.yml"

LOG="$(mktemp -t release-check-XXXXXX.log)"
trap 'rm -f "$LOG"' EXIT

fail() {
  echo "not ready: $*" >&2
  exit 1
}

step() { printf '  %-34s' "$1"; }
ok() { echo "ok"; }

# Run one gate, quietly. Output accumulates in $LOG across the whole run — the
# test-suite budget below reads the cargo output back out of it — and only the
# tail is shown, on failure.
gate() {
  local label="$1" remedy="$2"
  shift 2
  step "$label"
  if "$@" >>"$LOG" 2>&1; then
    ok
  else
    echo
    tail -40 "$LOG" >&2
    fail "$remedy"
  fi
}

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

# CI builds with the channel rust-toolchain.toml pins. A local rustc that
# differs makes every cargo gate below a measurement of the wrong compiler —
# the drift that let this script pass while CI failed on a newer clippy.
step "rustc matches rust-toolchain.toml"
pinned="$(grep -oP '^channel = "\K[^"]+' rust-toolchain.toml)" ||
  fail "no channel in rust-toolchain.toml"
local_rustc="$(rustc --version 2>>"$LOG" | awk '{print $2}')"
[ "$local_rustc" = "$pinned" ] || fail "local rustc is ${local_rustc:-missing}, rust-toolchain.toml pins $pinned.
  Enter the dev shell (nix-shell, or direnv) or let rustup read the pin. If the
  pin is what is stale, bump it — see the header of rust-toolchain.toml."
ok

# --- CI parity ---------------------------------------------------------------
#
# Every `- name:` step in ci.yml must appear in exactly one of these lists.
# REPRODUCED means this script runs the same command below; SETUP means the step
# provisions the GitHub runner and has no local equivalent (this machine already
# has a toolchain). Adding a CI step without deciding which it is fails here.
#
# CI_ONLY is the honest third answer, and it is deliberately awkward to use: a
# real gate that this script will not run, because running it would mean
# installing a whole second toolchain on the developer's machine. Today that is
# only the MSRV check, which needs the exact `rust-version` from Cargo.toml.
# Every entry here weakens the guarantee at the top of this file, so it is
# reported in the ready banner rather than passing silently — a release cut from
# here can still go red in CI on these, and the operator should know which.

CI_STEPS_REPRODUCED=(
  "Scanner selftest"
  "Scan tracked files"
  "Scan introduced history"
  "Scan full history (known count)"
  "Gitleaks (credentials)"
  "Cargo audit (advisories)"
  "npm audit (rhino-local lockfile)"
  "npm audit (shipped templates)"
  "Shellcheck (all scripts)"
  "Format"
  "Clippy (default)"
  "Test (default)"
  "Clippy (logs-store)"
  "Test (logs-store)"
  "Type tests (accept + reject)"
  "TypeScript project type-check"
  "rhino-local typecheck"
  "rhino-local lint"
  "rhino-local tests (host vs AM image)"
  "rhino-local package smoke (installed, no javac)"
)

CI_STEPS_SETUP=(
  "Install system dependencies"
  "Install Rust toolchain"
  "Cache cargo build"
  "Install Node"
  "Install TypeScript"
  "Install Java (AM's Temurin)"
  "Install rhino-local dependencies"
  "Pull the AM image"
  "Record the JVM builds"
  "Read the pinned toolchain"
  "Read the declared MSRV"
  "Install the declared MSRV toolchain"
  "Select the MSRV over the pinned toolchain"
)

# Asserts that the crate still builds on the `rust-version` Cargo.toml declares.
# Not run here: this machine has one toolchain, and the gate is only meaningful
# on the pinned one.
CI_STEPS_CI_ONLY=(
  "Check (default)"
  "Check (logs-store)"
)

step "ci.yml parity"
[ -f "$CI_YML" ] || fail "$CI_YML is missing — this script mirrors it and cannot check itself"
mapfile -t ci_steps < <(grep -oP '^      - name: \K.*' "$CI_YML")
[ "${#ci_steps[@]}" -gt 0 ] || fail "found no steps in $CI_YML — the parser needs updating"

known=("${CI_STEPS_REPRODUCED[@]}" "${CI_STEPS_SETUP[@]}" "${CI_STEPS_CI_ONLY[@]}")
in_list() {
  local needle="$1" item
  shift
  for item in "$@"; do [ "$item" = "$needle" ] && return 0; done
  return 1
}

for s in "${ci_steps[@]}"; do
  in_list "$s" "${known[@]}" || fail "$CI_YML has a step this script does not account for:
  \"$s\"
  Either run it below and add it to CI_STEPS_REPRODUCED, or add it to
  CI_STEPS_SETUP if it only provisions the runner. CI_STEPS_CI_ONLY is the last
  resort, for a gate that cannot run without a second toolchain — read its
  comment before reaching for it."
done
for s in "${known[@]}"; do
  in_list "$s" "${ci_steps[@]}" || fail "this script expects a CI step that no longer exists:
  \"$s\"
  It was removed or renamed in $CI_YML; update the lists here to match."
done
ok

# --- sensitive metadata ------------------------------------------------------
#
# REQUIRE_SENSITIVE_DENYLIST=1 matches CI, and matters more here than anywhere:
# without it the scanner silently runs its shape rules only, so a run with no
# denylist in the environment looks exactly like a clean one. .envrc exports
# SENSITIVE_DENYLIST; direnv, or `source .envrc`, puts it in scope.
export REQUIRE_SENSITIVE_DENYLIST=1
# Same for the known-history count: CI requires it, so a release does too. It
# lives beside the denylist (.envrc), because it is a count of that denylist's
# hits; 0 if the history is clean.
export REQUIRE_SENSITIVE_HISTORY_EXPECTED=1

if [ -z "${SENSITIVE_DENYLIST_CONTENT:-}" ] &&
  { [ -z "${SENSITIVE_DENYLIST:-}" ] || [ ! -f "${SENSITIVE_DENYLIST}" ]; }; then
  fail "no sensitive-metadata denylist in the environment.
  CI requires one and so does a release. Set SENSITIVE_DENYLIST to a file
  outside the repo (\`source .envrc\`, or let direnv do it)."
fi
if [ -z "${SENSITIVE_HISTORY_EXPECTED:-}" ]; then
  fail "SENSITIVE_HISTORY_EXPECTED is not set.
  CI requires it and so does a release: it is the number of findings your
  denylist already has in history (0 if none). Export it in .envrc."
fi

# --selftest first, for the reason ci.yml gives: a scanner whose rules silently
# stopped matching reports every tree clean forever.
gate "metadata: scanner selftest" \
  "the scanner's own rules do not all fire — fix the scanner before trusting a clean scan" \
  scripts/check-sensitive-metadata.sh --selftest

gate "metadata: tracked files" \
  "tenant or client metadata in tracked files (see the findings above)" \
  scripts/check-sensitive-metadata.sh --tracked

# Every blob a clone of this tag can reach, against the known count of historic
# findings — the same scan as CI's "Scan full history (known count)".
gate "metadata: full history" \
  "tenant or client metadata reachable in history (see the findings above)" \
  scripts/check-sensitive-metadata.sh --history

# --- gitleaks ----------------------------------------------------------------
#
# Orthogonal to the scanner above: that one knows the shape of tenant and
# client metadata, this one knows credentials. scripts/gitleaks.sh reads the version from ci.yml and fetches that exact
# release into the cache if it is not on PATH — the same binary the git hooks
# use.
step "gitleaks: resolve the pinned binary"
GITLEAKS_BIN="$(scripts/gitleaks.sh --path 2>>"$LOG")" || fail "could not obtain the pinned gitleaks.
  CI runs it, so a release cannot skip it. Install it on PATH at the version
  ci.yml pins, or make the download work and re-run."
ok

# --redact so a finding names the rule and file without printing the secret.
# .gitleaks.toml is picked up from the repo root automatically.
gate "gitleaks: credentials in history" \
  "gitleaks found credential material in history (see above).
  If it is a false positive, allowlist it by SHAPE in .gitleaks.toml — never by
  fingerprint, which pins a commit hash this repo has rewritten before." \
  "$GITLEAKS_BIN" git . --redact --no-banner --exit-code 1

# --- cargo audit -------------------------------------------------------------
#
# Same pinning as gitleaks: the version is read from ci.yml, and the static musl
# binary is fetched into the cache unless the exact version is already on PATH.
# Ignores, each with its reason, are in .cargo/audit.toml.
CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/pingone-aic-manager"
CARGO_AUDIT_VERSION="$(grep -oP '^\s+CARGO_AUDIT_VERSION:\s*\K\S+' "$CI_YML" | head -1)"
[ -n "$CARGO_AUDIT_VERSION" ] || fail "could not read CARGO_AUDIT_VERSION from $CI_YML"

CARGO_AUDIT_BIN="$CACHE/cargo-audit-$CARGO_AUDIT_VERSION"

if command -v cargo-audit >/dev/null 2>&1 &&
  [ "$(cargo-audit --version 2>/dev/null)" = "cargo-audit $CARGO_AUDIT_VERSION" ]; then
  CARGO_AUDIT_BIN="$(command -v cargo-audit)"
elif [ ! -x "$CARGO_AUDIT_BIN" ]; then
  step "cargo-audit: fetch $CARGO_AUDIT_VERSION"
  mkdir -p "$CACHE"
  tmp="$(mktemp -d)"
  name="cargo-audit-x86_64-unknown-linux-musl-v${CARGO_AUDIT_VERSION}"
  if curl -sSfL "https://github.com/rustsec/rustsec/releases/download/cargo-audit/v${CARGO_AUDIT_VERSION}/${name}.tgz" \
    | tar -xz -C "$tmp" "${name}/cargo-audit" >>"$LOG" 2>&1; then
    mv "$tmp/${name}/cargo-audit" "$CARGO_AUDIT_BIN"
    rm -rf "$tmp"
    ok
  else
    rm -rf "$tmp"
    echo
    fail "could not fetch cargo-audit $CARGO_AUDIT_VERSION.
  CI runs it, so a release cannot skip it. Install it on PATH at that exact
  version, or make the download work and re-run."
  fi
fi

gate "cargo audit: advisories" \
  "a dependency in Cargo.lock has a security advisory (see above). Update it
  (cargo update -p <crate>); ignore it in .cargo/audit.toml only when no fix
  exists, with the reason and the condition for removing the ignore." \
  "$CARGO_AUDIT_BIN" audit --deny warnings

# npm audit reads package-lock.json without node_modules. The shipped templates
# have no lockfile; scripts/npm-audit-templates.sh resolves and audits each.
gate "npm audit (rhino-local)" \
  "an npm dependency has a security advisory (see above)" \
  npm audit --audit-level=low

gate "npm audit (shipped templates)" \
  "a shipped template dependency has a security advisory (see above)" \
  scripts/npm-audit-templates.sh

# --- shell -------------------------------------------------------------------
#
# scripts/shellcheck-all.sh holds the severity and the not-yet-clean list, so CI
# and this script cannot drift into linting different files at different bars.

# Pinned in ci.yml, and the version matters: shellcheck releases disagree about
# both findings and the NOT_YET_CLEAN list, which is how CI sat red for a week
# while this script said ready. So a different local version is refused, not
# used.
SHELLCHECK_VERSION="$(grep -oP '^\s+SHELLCHECK_VERSION:\s*\K\S+' "$CI_YML" | head -1)"
[ -n "$SHELLCHECK_VERSION" ] || fail "could not read SHELLCHECK_VERSION from $CI_YML"
step "shellcheck version"
command -v shellcheck >/dev/null 2>&1 || fail "shellcheck is not on PATH; CI pins $SHELLCHECK_VERSION"
have="$(shellcheck --version | grep -oP '^version: \K\S+')"
[ "$have" = "$SHELLCHECK_VERSION" ] || fail "shellcheck $have is on PATH but CI pins $SHELLCHECK_VERSION.
  The two disagree about findings, so a green run here would not predict CI."
ok

gate "shellcheck" \
  "shellcheck findings in a tracked script (see above), or an exemption in
  scripts/shellcheck-all.sh that is no longer needed" \
  scripts/shellcheck-all.sh

# --- cargo -------------------------------------------------------------------

gate "cargo fmt" \
  "formatting differs (run: cargo fmt --all)" \
  cargo fmt --all -- --check

gate "cargo clippy" \
  "clippy warnings" \
  cargo clippy --all-targets -- -D warnings

gate "cargo test" \
  "tests failing" \
  cargo test

# The DuckDB local log-store is behind the opt-in `logs-store` feature. It rots
# silently without this: nothing in a default build compiles it.
gate "cargo clippy (logs-store)" \
  "clippy warnings under --features logs-store" \
  cargo clippy --all-targets --features logs-store -- -D warnings

gate "cargo test (logs-store)" \
  "tests failing under --features logs-store" \
  cargo test --features logs-store

# --- script workspace types --------------------------------------------------
#
# The .d.ts files under src/scripts/templates/ carry real logic, and no cargo
# gate compiles any of it — the Rust tests check the strings that emit those
# files, never what tsc makes of them.

command -v node >/dev/null 2>&1 || fail "node is not on PATH; CI type-checks the
  script workspace templates and a release cannot skip it."
command -v npm >/dev/null 2>&1 || fail "npm is not on PATH; CI type-checks the
  script workspace templates and a release cannot skip it."

# run.sh prefers a tsc on PATH and falls back to npx. CI installs typescript@5
# explicitly; here whichever is present is what a local `script watch` would
# use, so it is the faithful thing to gate on.
gate "script template type tests" \
  "the shipped .d.ts declarations do not type-check (accept/reject fixtures)" \
  scripts/type-tests/run.sh

# `npm run type-check` is the project's own entry point and runs BOTH programs —
# the endpoint program under the narrow IDM runtime lib, the tests under node.
gate "typescript project type-check" \
  "the TypeScript endpoint project does not type-check" \
  bash -c 'cd src/scripts/templates/typescript &&
           npm install --no-audit --no-fund &&
           npm run type-check'

# --- rhino-local harness -----------------------------------------------------
#
# CI runs every harness test on the host JVM and the AM image's JVM and fails on
# a difference, so this does too: a release checked only on the host lane has
# not checked the thing CI checks. It needs a Java 25 (shell.nix provides one)
# and docker with the AM image; the harness says which is missing.

gate "rhino-local typecheck + lint" \
  "the rhino-local harness does not type-check or lint clean" \
  bash -c 'npm ci --no-audit --no-fund &&
           npm -w packages/rhino-local run typecheck &&
           npm -w packages/rhino-local run lint &&
           npm -w packages/rhino-local run lint:am'

gate "rhino-local tests (host vs AM image)" \
  "the rhino-local harness failed, or the host and AM image JVMs disagreed" \
  bash -c 'AIC_SCRIPT_TESTER_JVM=both npm -w packages/rhino-local test'

gate "rhino-local package smoke (installed, no javac)" \
  "the packed rhino-local does not install and run outside this checkout" \
  scripts/rhino-local-pack-smoke.sh

# --- budgets -----------------------------------------------------------------

# Wall-clock budget on the unit suite. The grep guard in src/lib.rs catches
# cryptographic keygen called DIRECTLY from a test module; it cannot see a test
# that calls a production helper which generates a key one level down, which is
# exactly how the 2026-08-06 instance got in (0.33s -> 8.31s). This catches the
# transitive case, and anything else that quietly makes the suite slow.
#
# Budget is deliberately loose — ~10x the current ~0.3s — so it flags a step
# change, not ordinary growth. Raise it when the suite legitimately outgrows it,
# but read why it grew first.
BUDGET_MS=3000
slowest="$(awk '/^test result: ok/ {
  for (i = 1; i <= NF; i++)
    if ($i == "in") { gsub(/s$/, "", $(i + 1)); if ($(i + 1) + 0 > max) max = $(i + 1) + 0 }
} END { printf "%d", max * 1000 }' "$LOG")"
if [ "${slowest:-0}" -gt "$BUDGET_MS" ]; then
  fail "unit suite took ${slowest}ms, over the ${BUDGET_MS}ms budget.
  Usually cryptographic key generation reachable from a test — assert against a
  stub instead, or gate the real thing behind #[ignore]. See REVIEW.md."
fi

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
  tests passing     $passed (across $tests binaries, both feature sets)
  not run here      ${#CI_STEPS_CI_ONLY[@]} CI-only gate(s): ${CI_STEPS_CI_ONLY[*]}
                    (MSRV = $(grep -m1 '^rust-version = ' Cargo.toml | cut -d'"' -f2); CI checks it, this script does not)

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
