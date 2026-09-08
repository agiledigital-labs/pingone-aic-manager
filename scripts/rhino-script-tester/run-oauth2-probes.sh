#!/usr/bin/env bash
# Probe runner for the next-gen OAUTH2_VALIDATE_SCOPE context.
#
# The journey harness next door cannot reach this context: a validate-scope
# script is invoked by a token request, not by an authentication tree. So this
# lane drives a throwaway `client_credentials` client whose
# `overrideOAuth2ClientConfig` points at one throwaway script, swaps the script
# body per fixture, and records what the CLIENT sees. Never realm-wide — the
# realm default would apply to every client in the tenant.
#
# What each fixture proves is the HTTP status and the `scope` the client is
# handed back, so a denial that still issues a token is visible as such.
#
# Usage:
#   scripts/rhino-script-tester/run-oauth2-probes.sh [fixture.js ...]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

TENANT="${TENANT:-sandbox}"
REALM="${REALM:-alpha}"
AIC_BIN="${AIC_BIN:-$ROOT/target/debug/aic}"
CLIENT_ID="${CLIENT_ID:-aic-probe-scopevalidator}"
SCRIPT_REF="${SCRIPT_REF:-$REALM/AIC-ScopeValidator-Probe}"
WORKSPACE_CJS="${WORKSPACE_CJS:-$ROOT/workspace/$TENANT/am/$REALM/oauth2-validate-scope-ng/AIC-ScopeValidator-Probe.cjs}"
# Two scopes, deliberately: `deny-narrowed-list-partial` needs one scope to
# survive the narrowing, and a single-scope request hides the dangerous case
# behind a clean 403.
SCOPE="${SCOPE:-aicedit-probe aicedit-probe-keep}"
FIXTURES_DIR="${FIXTURES_DIR:-$SCRIPT_DIR/fixtures-oauth2}"
OUT_DIR="${OUT_DIR:-$ROOT/tmp/rhino-script-tester}"

# Never hard-code the tenant host: derive it, so nothing here carries a
# client-identifying hostname into the repo.
BASE="${BASE:-$("$AIC_BIN" ctx list --no-prompt | awk '/^\*/{print $4}')}"
[[ -n "$BASE" ]] || { echo "Could not resolve tenant base URL" >&2; exit 1; }

SECRET_FILE="${SECRET_FILE:-}"
[[ -n "$SECRET_FILE" && -r "$SECRET_FILE" ]] || {
  echo "Set SECRET_FILE to a file holding the probe client's secret" >&2; exit 1; }
SECRET="$(cat "$SECRET_FILE")"

need() { command -v "$1" >/dev/null 2>&1 || { echo "Missing: $1" >&2; exit 1; }; }
need curl; need jq

mkdir -p "$OUT_DIR"
RESULTS="$OUT_DIR/oauth2-probe-results.json"

if [[ $# -gt 0 ]]; then fixtures=("$@"); else
  mapfile -t fixtures < <(ls "$FIXTURES_DIR"/*.script.js | sort); fi

tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
echo "[]" > "$RESULTS"

for fixture in "${fixtures[@]}"; do
  name="$(basename "$fixture" .script.js)"
  printf '=== %s\n' "$name"

  cp "$fixture" "$WORKSPACE_CJS"
  if ! "$AIC_BIN" script push "$SCRIPT_REF" --no-prompt >"$tmp/push.log" 2>&1; then
    echo "  push FAILED"; sed 's/^/    /' "$tmp/push.log"; continue
  fi

  # The script body is read per token request, but give AM a moment to settle.
  sleep 2

  status="$(curl -sS -o "$tmp/tok.json" -D "$tmp/tok.hdr" -w '%{http_code}' \
    -X POST "$BASE/am/oauth2/realms/root/realms/$REALM/access_token" \
    --data-urlencode "grant_type=client_credentials" \
    --data-urlencode "client_id=$CLIENT_ID" \
    --data-urlencode "client_secret=$SECRET" \
    --data-urlencode "scope=$SCOPE")"
  txid="$(awk -F': ' 'tolower($1)=="x-forgerock-transactionid"{gsub(/\r/,"",$2);print $2}' "$tmp/tok.hdr")"

  granted="$(jq -r 'if has("scope") then (.scope|tostring) else "<no scope field>" end' "$tmp/tok.json" 2>/dev/null || echo "<unparseable>")"
  err="$(jq -r '[.error // empty, .error_description // empty] | map(select(. != "")) | join(": ")' "$tmp/tok.json" 2>/dev/null || echo "")"
  issued="$(jq -r 'if has("access_token") then "yes" else "no" end' "$tmp/tok.json" 2>/dev/null || echo "no")"

  printf '  http=%s token_issued=%s scope=%s %s\n' "$status" "$issued" "$granted" "${err:+error=$err}"

  jq --arg f "$name" --arg s "$status" --arg i "$issued" --arg sc "$granted" \
     --arg e "$err" --arg t "$txid" \
     '. += [{fixture:$f, http:$s, token_issued:$i, scope:$sc, error:$e, txid:$t}]' \
     "$RESULTS" > "$RESULTS.tmp" && mv "$RESULTS.tmp" "$RESULTS"
done

echo
echo "Results: $RESULTS"
jq -r '.[] | "\(.http)  issued=\(.token_issued)  scope=\(.scope)  \(.fixture)"' "$RESULTS"
