#!/usr/bin/env bash
# Type-check the shipped script-workspace declarations, in both directions.
# See README.md. Exits non-zero on the first leaf that misbehaves.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HERE="$ROOT/scripts/type-tests"
TEMPLATES="$ROOT/src/scripts/templates"

if command -v tsc >/dev/null 2>&1; then
  TSC=(tsc)
elif command -v npx >/dev/null 2>&1; then
  # `npx typescript@5 tsc` does NOT work: the package ships two binaries (tsc
  # and tsserver), npx cannot infer which to run, and the trailing `tsc` is
  # read as an argument — it fails with "could not determine executable to
  # run". Name the package explicitly. Neither CI nor a normal dev box reaches
  # this branch (both have tsc on PATH), which is exactly why it was broken and
  # green for a whole review.
  TSC=(npx --yes --package typescript@5 -- tsc)
else
  echo "type-tests: need tsc or npx on PATH" >&2
  exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# The AM and IDM workspaces have separate declaration sets that deliberately
# redeclare the same global names, so they cannot share one program — the same
# reason each shipped workspace has a base tsconfig with `"files": []` and a
# leaf per script family.
for ws in am idm; do
  mkdir -p "$work/$ws/types"
  cp "$TEMPLATES/$ws"/types/*.d.ts "$work/$ws/types/"
  cp "$TEMPLATES/$ws"/tsconfig.json "$work/$ws/tsconfig.json"
done

fail=0

run_tsc() { "${TSC[@]}" -p "$1" 2>&1; }

for leaf in "$HERE"/leaves/*/; do
  name="$(basename "$leaf")"
  ws="$(tr -d '[:space:]' < "$leaf/workspace")"
  if [ ! -d "$work/$ws" ]; then
    echo "type-tests: $name names an unknown workspace: $ws" >&2
    fail=1
    continue
  fi

  # `types` is a newline-separated list of declaration files, one per line: a
  # SUBSET of the include set the Rust leaf_tsconfig emits for this family. The
  # Rust test type_test_leaf_manifests_are_subsets_of_the_real_leaf_configs is
  # what stops it from becoming a set no shipped workspace has.
  mapfile -t types < "$leaf/types"
  includes='"./**/*"'
  for t in "${types[@]}"; do
    [ -n "$t" ] || continue
    if [ ! -f "$work/$ws/types/$t" ]; then
      echo "type-tests: $name names a declaration that does not exist: $ws/types/$t" >&2
      fail=1
    fi
    includes="$includes, \"../types/$t\""
  done

  dir="$work/$ws/$name"
  mkdir -p "$dir"
  printf '{\n  "extends": "../tsconfig.json",\n  "include": [%s]\n}\n' "$includes" \
    > "$dir/tsconfig.json"

  # --- accept: must compile clean -----------------------------------------
  cp "$leaf/accept.cjs" "$dir/accept.cjs"
  if out="$(run_tsc "$dir/tsconfig.json")"; then
    echo "ok   $name accept"
  else
    echo "FAIL $name: accept.cjs did not compile" >&2
    echo "$out" >&2
    fail=1
  fi
  rm "$dir/accept.cjs"

  # --- reject: every marked line must fail, and only marked lines ----------
  cp "$leaf/reject.cjs" "$dir/reject.cjs"
  out="$(run_tsc "$dir/tsconfig.json" || true)"

  # Lines the fixture says must fail, and with which diagnostic.
  expected="$(grep -n 'expect:' "$leaf/reject.cjs" \
    | sed -E 's/^([0-9]+):.*expect:[[:space:]]*([A-Z0-9]+).*/\1 \2/')"
  if [ -z "$expected" ]; then
    echo "FAIL $name: reject.cjs has no // expect: markers" >&2
    fail=1
  fi

  # Every diagnostic tsc produced, as "line CODE" — and the reject phase must
  # account for ALL of them, not just the ones from reject.cjs.
  #
  # tsc prints the path relative to the CWD it was invoked from, which is not
  # the leaf directory, so the basename is matched anywhere on the line and
  # never anchored. An anchored pattern here matched nothing and the harness
  # reported every reject case as "got: nothing" while tsc was in fact rejecting
  # all of them: a green-looking gate that had checked zero of its assertions.
  actual="$(printf '%s\n' "$out" \
    | sed -nE 's#.*reject\.cjs\(([0-9]+),[0-9]+\): error (TS[0-9]+).*#\1 \2#p' \
    | sort -u)"

  # Diagnostics from anywhere ELSE — a declaration file, the tsconfig, a
  # location-less config error. The accept run usually catches these, but a
  # conditional type that only blows up on an input the reject file supplies
  # would produce one alongside the expected call-site error and go unnoticed.
  foreign="$(printf '%s\n' "$out" \
    | grep -E 'error TS[0-9]+' \
    | grep -v 'reject\.cjs(' || true)"

  leaf_fail=0

  if [ -n "$foreign" ]; then
    echo "FAIL $name: reject.cjs run produced diagnostics outside reject.cjs:" >&2
    printf '%s\n' "$foreign" >&2
    leaf_fail=1
  fi

  while read -r line code; do
    [ -n "$line" ] || continue
    if ! printf '%s\n' "$actual" | grep -qx "$line $code"; then
      got="$(printf '%s\n' "$actual" | grep "^$line " || echo 'nothing')"
      echo "FAIL $name: reject.cjs:$line expected $code, got: $got" >&2
      leaf_fail=1
    fi
  done <<< "$expected"

  # The reverse direction, matched on the exact PAIR. Matching the line alone
  # let a marked line smuggle in a second, unrequested diagnostic — so a type
  # that rejected the right line for an entirely wrong reason passed.
  while read -r line code; do
    [ -n "$line" ] || continue
    if ! printf '%s\n' "$expected" | grep -qx "$line $code"; then
      echo "FAIL $name: reject.cjs:$line produced an unrequested $code — a type that rejects the wrong thing, or the right thing for the wrong reason, breaks working scripts" >&2
      leaf_fail=1
    fi
  done <<< "$actual"

  if [ "$leaf_fail" -eq 0 ]; then
    echo "ok   $name reject"
  else
    fail=1
  fi
  rm "$dir/reject.cjs"
done

if [ "$fail" -ne 0 ]; then
  echo "type-tests: FAILED" >&2
  exit 1
fi
echo "type-tests: all leaves passed"
