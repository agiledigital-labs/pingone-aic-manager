#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

TENANT="${TENANT:-sandbox}"
BASE="${BASE:-https://tenant.example.com}"
REALM_PATH="${REALM_PATH:-/am/json/realms/root/realms/alpha}"
AIC_BIN="${AIC_BIN:-$ROOT/target/debug/aic}"

SCRIPT_SOURCE="${1:-$SCRIPT_DIR/scripts/rhino-let-behaviour.script.js}"
SCRIPT_NAME="${SCRIPT_NAME:-AIC Rhino Let Probe}"
SCRIPT_ID="${SCRIPT_ID:-2e87a29c-0e30-4d85-bf0e-a1c0a11e7001}"
EVALUATOR_VERSION="${EVALUATOR_VERSION:-2.0}"

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

need base64
need curl
need jq

if [[ ! -x "$AIC_BIN" ]]; then
  echo "Missing aic binary: $AIC_BIN" >&2
  echo "Build it first with: cargo build --locked --offline" >&2
  exit 1
fi

if [[ ! -f "$SCRIPT_SOURCE" ]]; then
  echo "Missing script source: $SCRIPT_SOURCE" >&2
  exit 1
fi

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

TOKEN="$("$AIC_BIN" whoami --tenant "$TENANT" --token)"
if [[ -z "$TOKEN" ]]; then
  echo "Failed to obtain bearer token from aic agent" >&2
  exit 1
fi

curl_common=(
  -H "Authorization: Bearer $TOKEN"
  -H "Accept-API-Version: protocol=2.0,resource=1.0"
  -H "Content-Type: application/json"
)

curl_json() {
  local method="$1"
  local url="$2"
  local payload="${3:-}"
  local response="$tmpdir/curl-response.json"
  local status

  if [[ -n "$payload" ]]; then
    status="$(curl -sS -o "$response" -w '%{http_code}' -X "$method" "${curl_common[@]}" --data @"$payload" "$url")"
  else
    status="$(curl -sS -o "$response" -w '%{http_code}' -X "$method" "${curl_common[@]}" "$url")"
  fi

  cat "$response"

  if [[ "$status" != 2* ]]; then
    echo "API $method $url returned HTTP $status" >&2
    jq . "$response" >&2 || cat "$response" >&2
    return 1
  fi
}

urlencode() {
  jq -nr --arg value "$1" '$value | @uri'
}

SCRIPT_B64="$(base64 -w0 "$SCRIPT_SOURCE")"
encoded_script_name="$(urlencode "$SCRIPT_NAME")"

existing_script="$(
  curl_json GET "$BASE$REALM_PATH/scripts?_queryFilter=name+eq+%22$encoded_script_name%22" \
    | jq -r '.result[0]._id // empty'
)"
if [[ -n "$existing_script" ]]; then
  SCRIPT_ID="$existing_script"
fi

jq -n \
  --arg id "$SCRIPT_ID" \
  --arg name "$SCRIPT_NAME" \
  --arg script "$SCRIPT_B64" \
  --arg ev "$EVALUATOR_VERSION" \
  '{
    _id: $id,
    name: $name,
    description: "AIC Rhino behavior probe. Safe to delete.",
    script: $script,
    default: false,
    language: "JAVASCRIPT",
    context: "AUTHENTICATION_TREE_DECISION_NODE",
    evaluatorVersion: $ev
  }' > "$tmpdir/script.json"

curl_json PUT "$BASE$REALM_PATH/scripts/$SCRIPT_ID" "$tmpdir/script.json" > "$tmpdir/script.out.json"
echo "Script updated: $SCRIPT_NAME ($SCRIPT_ID)"
echo "Source: $SCRIPT_SOURCE"
