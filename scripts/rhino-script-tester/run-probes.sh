#!/usr/bin/env bash
# Batch probe runner: uploads each fixture to the one sandbox probe script,
# invokes the probe journey, and records structured results.
#
# A fixture that PARSES + RUNS returns a HiddenValueCallback with JSON
# `{ ok, feature, value | error }`. A fixture that fails to PARSE returns no
# callback at all (this is how we detected the `let` failure) — recorded here as
# `callback: "no-callback"`, which for a single-feature fixture means a parse
# error. Set FETCH_LOGS=1 (with log API keys in .envrc) to also pull per-fixture
# transaction logs for the parse-error text.
#
# Usage:
#   scripts/rhino-script-tester/run-probes.sh                 # all fixtures
#   scripts/rhino-script-tester/run-probes.sh fixtures/arrow-function.script.js ...
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

BASE="${BASE:-https://tenant.example.com}"
REALM_PATH="${REALM_PATH:-/am/json/realms/root/realms/alpha}"
TREE_NAME="${TREE_NAME:-AIC-Rhino-Let-Probe}"
AUTH_TIMEOUT_SECONDS="${AUTH_TIMEOUT_SECONDS:-30}"
FIXTURES_DIR="${FIXTURES_DIR:-$SCRIPT_DIR/fixtures}"
OUT_DIR="${OUT_DIR:-$ROOT/tmp/rhino-script-tester}"
FETCH_LOGS="${FETCH_LOGS:-0}"

need() { command -v "$1" >/dev/null 2>&1 || { echo "Missing required command: $1" >&2; exit 1; }; }
need base64; need curl; need jq

mkdir -p "$OUT_DIR"
RESULTS="$OUT_DIR/probe-results.json"

urlencode() { jq -nr --arg value "$1" '$value | @uri'; }

if [[ $# -gt 0 ]]; then
  fixtures=("$@")
else
  mapfile -t fixtures < <(ls "$FIXTURES_DIR"/*.script.js | sort)
fi

encoded_tree_name="$(urlencode "$TREE_NAME")"
auth_url="$BASE$REALM_PATH/authenticate?authIndexType=service&authIndexValue=$encoded_tree_name"

tmpdir="$(mktemp -d)"; trap 'rm -rf "$tmpdir"' EXIT
results_json="[]"

for fx in "${fixtures[@]}"; do
  [[ "$fx" = /* ]] || fx="$SCRIPT_DIR/$fx"
  name="$(basename "$fx" .script.js)"
  echo "=== probe: $name ==="

  "$SCRIPT_DIR/update-script.sh" "$fx" >/dev/null

  body="$tmpdir/body.json"; headers="$tmpdir/headers"
  status="$(curl -sS --max-time "$AUTH_TIMEOUT_SECONDS" -o "$body" -D "$headers" -w '%{http_code}' \
    -X POST \
    -H "Accept-API-Version: resource=2.0, protocol=1.0" \
    -H "Content-Type: application/json" \
    "$auth_url" || echo "000")"
  txid="$(awk 'tolower($1)=="x-forgerock-transactionid:" {print $2}' "$headers" | tr -d '\r' | tail -n 1)"
  hidden="$(jq -r '.callbacks[]? | select(.type=="HiddenValueCallback") | .output[]? | select(.name=="value") | .value' "$body" 2>/dev/null | head -n 1)"

  if [[ -n "$hidden" && "$hidden" != "null" ]]; then
    callback="parsed"
    payload="$hidden"
  else
    callback="no-callback"
    payload="$(jq -c '.' "$body" 2>/dev/null || echo '{}')"
    if [[ "$FETCH_LOGS" == "1" && -n "$txid" ]]; then
      LOG_OUTPUT="$OUT_DIR/logs-$name.json" "$SCRIPT_DIR/get-transaction-logs.sh" "$txid" >/dev/null 2>&1 || true
    fi
  fi

  echo "  status=$status callback=$callback txid=${txid:-none}"
  echo "  payload: $payload"

  payload_json="$(printf '%s' "$payload" | jq -c '.' 2>/dev/null || printf '%s' "$payload" | jq -Rc '.')"
  results_json="$(jq -c \
    --arg name "$name" --arg status "$status" --arg callback "$callback" --arg txid "${txid:-}" \
    --argjson payload "$payload_json" \
    '. + [{name:$name, httpStatus:$status, callback:$callback, transactionId:$txid, payload:$payload}]' \
    <<<"$results_json")"
done

echo "$results_json" | jq '.' > "$RESULTS"
echo
echo "Wrote $RESULTS"
echo "=== summary ==="
echo "$results_json" | jq -r '.[] | "\(.name): http=\(.httpStatus) callback=\(.callback)"'
