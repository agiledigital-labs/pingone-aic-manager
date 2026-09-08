#!/usr/bin/env bash
# Invoke the probe journey with DUPLICATED request headers and DUPLICATED query
# parameters, so `fixtures/request-multivalue.script.js` can report how many
# elements each binding value holds. Everything else matches run-journey.sh.
set -euo pipefail

BASE="${BASE:-https://tenant.example.com}"
REALM_PATH="${REALM_PATH:-/am/json/realms/root/realms/alpha}"
TREE_NAME="${TREE_NAME:-AIC-Rhino-Let-Probe}"
AUTH_TIMEOUT_SECONDS="${AUTH_TIMEOUT_SECONDS:-30}"

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

need curl
need jq

urlencode() {
  jq -nr --arg value "$1" '$value | @uri'
}

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

headers="$tmpdir/auth.headers"
body="$tmpdir/auth.body.json"
encoded_tree_name="$(urlencode "$TREE_NAME")"

# Same name twice, once, and once comma-joined — for both headers and query.
probe_headers=(
  -H "X-Aic-Probe: alpha"
  -H "X-Aic-Probe: bravo"
  -H "X-Aic-Probe-Single: solo"
  -H "X-Aic-Probe-Joined: alpha,bravo"
  -H "Accept-Language: en"
  -H "Accept-Language: fr"
)
probe_query="probeq=alpha&probeq=bravo&probeqsingle=solo&probeqjoined=alpha%2Cbravo"

echo "Invoking journey with duplicated headers/params: $TREE_NAME"
status="$(
  curl -sS --max-time "$AUTH_TIMEOUT_SECONDS" -o "$body" -D "$headers" -w '%{http_code}' \
    -X POST \
    -H "Accept-API-Version: resource=2.0, protocol=1.0" \
    -H "Content-Type: application/json" \
    "${probe_headers[@]}" \
    "$BASE$REALM_PATH/authenticate?authIndexType=service&authIndexValue=$encoded_tree_name&$probe_query"
)"

txid="$(awk 'tolower($1)=="x-forgerock-transactionid:" {print $2}' "$headers" | tr -d '\r' | tail -n 1)"
echo "HTTP status: $status"
if [[ -n "$txid" ]]; then
  echo "Transaction ID: $txid"
fi

if [[ "$status" != 2* ]]; then
  jq . "$body" || cat "$body"
  exit 1
fi

hidden="$(
  jq -r '.callbacks[]? | select(.type == "HiddenValueCallback") | .output[] | select(.name == "value") | .value' "$body" \
    | head -n 1
)"

if [[ -z "$hidden" || "$hidden" == "null" ]]; then
  echo "No HiddenValueCallback result returned"
  jq . "$body"
  exit 1
fi

echo "$hidden" | jq .
