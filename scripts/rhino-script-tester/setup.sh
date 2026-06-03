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
TREE_NAME="${TREE_NAME:-AIC-Rhino-Let-Probe}"
SCRIPT_ID="${SCRIPT_ID:-2e87a29c-0e30-4d85-bf0e-a1c0a11e7001}"
NODE_ID="${NODE_ID:-2e87a29c-0e30-4d85-bf0e-a1c0a11e7002}"
SUCCESS_NODE_ID="${SUCCESS_NODE_ID:-70e691a5-1e33-4ac3-a356-e7b6d60d92e0}"
FAILURE_NODE_ID="${FAILURE_NODE_ID:-e301438c-0bd0-429c-ab0c-66126501069a}"

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

echo "Preparing scripted decision probe resources"
echo "Script source: $SCRIPT_SOURCE"

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
  '{
    _id: $id,
    name: $name,
    description: "AIC Rhino let behavior probe. Safe to delete.",
    script: $script,
    default: false,
    language: "JAVASCRIPT",
    context: "AUTHENTICATION_TREE_DECISION_NODE",
    evaluatorVersion: "2.0"
  }' > "$tmpdir/script.json"

curl_json PUT "$BASE$REALM_PATH/scripts/$SCRIPT_ID" "$tmpdir/script.json" > "$tmpdir/script.out.json"
echo "Script ready: $SCRIPT_NAME ($SCRIPT_ID)"

jq -n \
  --arg id "$NODE_ID" \
  --arg script "$SCRIPT_ID" \
  '{
    _id: $id,
    _type: {
      _id: "ScriptedDecisionNode",
      collection: true,
      name: "Scripted Decision"
    },
    _outcomes: [
      { id: "ok", displayName: "ok" },
      { id: "error", displayName: "error" }
    ],
    inputs: ["*"],
    outcomes: ["ok", "error"],
    outputs: ["*"],
    script: $script
  }' > "$tmpdir/node.json"

curl_json PUT "$BASE$REALM_PATH/realm-config/authentication/authenticationtrees/nodes/ScriptedDecisionNode/$NODE_ID" "$tmpdir/node.json" > "$tmpdir/node.out.json"
echo "Journey node ready: $NODE_ID"

jq -n \
  --arg node "$NODE_ID" \
  --arg success "$SUCCESS_NODE_ID" \
  --arg failure "$FAILURE_NODE_ID" \
  --arg display "$SCRIPT_NAME" \
  '{
    identityResource: "managed/alpha_user",
    entryNodeId: $node,
    innerTreeOnly: false,
    description: "AIC Rhino let behavior probe. Safe to delete.",
    noSession: false,
    mustRun: false,
    enabled: true,
    transactionalOnly: false,
    uiConfig: {
      categories: "[\"Test\",\"Unit\"]"
    },
    nodes: {
      ($node): {
        connections: {
          ok: $success,
          error: $failure
        },
        displayName: $display,
        nodeType: "ScriptedDecisionNode",
        version: "1.0",
        x: 300,
        y: 200
      }
    }
  }' > "$tmpdir/tree.json"

curl_json PUT "$BASE$REALM_PATH/realm-config/authentication/authenticationtrees/trees/$TREE_NAME" "$tmpdir/tree.json" > "$tmpdir/tree.out.json"
echo "Journey ready: $TREE_NAME"
echo "Setup complete. For normal iterations, run scripts/rhino-script-tester/update-script.sh and scripts/rhino-script-tester/run-journey.sh."
