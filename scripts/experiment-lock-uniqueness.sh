#!/usr/bin/env bash
# experiment-lock-uniqueness.sh
#
# Question this answers:
#   If we create an IDM managed object with a *client-specified* _id as a "lock"
#   (PUT /openidm/managed/{type}/{id} with `If-None-Match: *`), does IDM reliably
#   reject all-but-one of N concurrent creates of the same _id in the target
#   environment? I.e. does this environment honor the conditional create path
#   well enough for an advisory reconById guard?
#
# Method:
#   For each round, pick a fresh _id, then fire N PUT requests in parallel, aligned
#   on a busy-wait barrier so they hit the server as simultaneously as possible.
#   Tally HTTP codes. The expected single-node result is exactly one 201
#   (winner) and N-1 412 (Precondition Failed / "Entry Already Exists") every
#   round, never a second 2xx.
#
# Token: taken from the running `aic agent` (./target/debug/aic whoami --token).
#   No pyjwt / verify-endpoint.sh needed.
#
# The create-if-absent primitive was verified live 2026-06-09 against alpha_role:
#   201 on first PUT (returns instance _rev); 412 "Entry Already Exists" on dup,
#   enforced by the DJ directory backend (uid=...,ou=role) — an atomic LDAP add,
#   not an IDM read-then-write. That backend-level atomicity is what this script
#   stress-tests. Later clustered-prod testing showed the PUT precondition is not
#   always honored there (200 silent updates are possible), so a passing sandbox
#   run is evidence, not proof of a production-safe distributed lock.
#
# Usage:
#   scripts/experiment-lock-uniqueness.sh                 # 20 parallel x 10 rounds, alpha_role
#   PARALLEL=50 ROUNDS=20 scripts/experiment-lock-uniqueness.sh
#   TYPE=alpha_organization scripts/experiment-lock-uniqueness.sh
#
# Env:
#   PARALLEL  concurrent creates per round   (default 20)
#   ROUNDS    number of rounds               (default 10)
#   TYPE      managed object type            (default alpha_role; needs only `name`)
#   PREFIX    _id prefix for test objects    (default lockexp)
#   KEEP=1    skip cleanup (leave objects behind for inspection)

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
AIC="$ROOT/target/debug/aic"

PARALLEL="${PARALLEL:-20}"
ROUNDS="${ROUNDS:-10}"
TYPE="${TYPE:-alpha_role}"
PREFIX="${PREFIX:-lockexp}"

# --- tenant base url (from .envrc; no secrets echoed) ---
if [ -z "${TENANT_BASE_URL:-}" ] && [ -f "$ROOT/.envrc" ]; then
  TENANT_BASE_URL="$(grep -oE 'https://[^"]+forgeblocks\.com' "$ROOT/.envrc" | head -1 || true)"
fi
if [ -z "${TENANT_BASE_URL:-}" ]; then
  echo "error: could not determine TENANT_BASE_URL (set it or check .envrc)" >&2
  exit 2
fi
BASE="${TENANT_BASE_URL%/}"

# --- token from the running agent ---
if [ ! -x "$AIC" ]; then
  echo "error: $AIC not found — build it (cargo build) or check the path" >&2
  exit 2
fi
TOKEN=${TOKEN:-"$("$AIC" whoami --token 2>/dev/null || true)"}
if [ -z "$TOKEN" ]; then
  echo "error: failed to get a token from the agent. Is it running and unlocked?" >&2
  echo "       check: $AIC status   (and: $AIC login)" >&2
  exit 2
fi

APIVER="Accept-API-Version: resource=1.0"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "tenant:    $BASE"
echo "type:      $TYPE"
echo "parallel:  $PARALLEL   rounds: $ROUNDS"
echo "id prefix: $PREFIX"
echo

# Fire one create. $1 = full _id, $2 = worker index, $3 = start-flag path.
# Busy-waits on the flag so all workers in a round release together.
worker() {
  local id="$1" idx="$2" flag="$3"
  while [ ! -f "$flag" ]; do :; done
  curl -sS -o "$TMP/body.$idx" -w '%{http_code}' -X PUT \
    -H "Authorization: Bearer $TOKEN" \
    -H "$APIVER" \
    -H "Content-Type: application/json" \
    -H "If-None-Match: *" \
    --data "{\"name\":\"$id\"}" \
    "$BASE/openidm/managed/$TYPE/$id" \
    > "$TMP/code.$idx" 2>"$TMP/curlerr.$idx" || echo "curl-fail" > "$TMP/code.$idx"
}

total_201=0
total_412=0
total_other=0
bad_rounds=0

for r in $(seq 1 "$ROUNDS"); do
  id="${PREFIX}-r${r}-$$"
  flag="$TMP/go.$r"
  rm -f "$TMP/code".* "$TMP/body".* "$TMP/curlerr".* 2>/dev/null || true

  # launch all workers; they block on the flag
  pids=()
  for i in $(seq 1 "$PARALLEL"); do
    worker "$id" "$i" "$flag" &
    pids+=("$!")
  done
  # give them a moment to reach the busy-wait, then release simultaneously
  sleep 0.15
  : > "$flag"
  for p in "${pids[@]}"; do wait "$p"; done

  # tally
  c201=0; c412=0; cother=0; other_detail=""
  for i in $(seq 1 "$PARALLEL"); do
    code="$(cat "$TMP/code.$i" 2>/dev/null || echo '???')"
    case "$code" in
      201) c201=$((c201+1)) ;;
      412) c412=$((c412+1)) ;;
      *)   cother=$((cother+1))
           snippet="$(head -c 160 "$TMP/body.$i" 2>/dev/null | tr -d '\n')"
           other_detail="${other_detail}    [worker $i] HTTP $code :: ${snippet}\n" ;;
    esac
  done

  total_201=$((total_201+c201)); total_412=$((total_412+c412)); total_other=$((total_other+cother))

  verdict="ok"
  if [ "$c201" -ne 1 ] || [ "$cother" -ne 0 ]; then
    verdict="ANOMALY"
    bad_rounds=$((bad_rounds+1))
  fi
  printf 'round %2d: 201=%-3d 412=%-3d other=%-3d  -> %s\n' "$r" "$c201" "$c412" "$cother" "$verdict"
  if [ -n "$other_detail" ]; then
    printf '%b' "$other_detail"
  fi

  # cleanup this round's object (the one winner)
  if [ "${KEEP:-0}" != "1" ]; then
    curl -sS -o /dev/null -X DELETE \
      -H "Authorization: Bearer $TOKEN" -H "$APIVER" \
      "$BASE/openidm/managed/$TYPE/$id" || true
  fi
done

echo
echo "=== summary over $ROUNDS rounds x $PARALLEL parallel ==="
echo "total 201 (created): $total_201   (expected: $ROUNDS, one winner per round)"
echo "total 412 (rejected): $total_412   (expected: $((ROUNDS*(PARALLEL-1))))"
echo "total other:          $total_other   (expected: 0)"
echo "anomalous rounds:     $bad_rounds   (expected: 0)"
echo
if [ "$bad_rounds" -eq 0 ] && [ "$total_other" -eq 0 ] && [ "$total_201" -eq "$ROUNDS" ]; then
  echo "RESULT: environment passed: exactly one create won every round; duplicates 412."
else
  echo "RESULT: environment failed: see anomalous rounds above. A PUT-based lock is not safe as-is."
  exit 1
fi
