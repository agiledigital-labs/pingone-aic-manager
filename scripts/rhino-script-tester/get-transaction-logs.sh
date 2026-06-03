#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

BASE="${BASE:-${ORIGIN:-https://tenant.example.com}}"
LOG_API_KEY_ID="${LOG_API_KEY_ID:-${API_KEY_ID:-}}"
LOG_API_KEY_SECRET="${LOG_API_KEY_SECRET:-${API_KEY_SECRET:-}}"
LOG_OUTPUT="${LOG_OUTPUT:-$ROOT/tmp/rhino-script-tester/logs.json}"

: "${LOG_API_KEY_ID:?Set LOG_API_KEY_ID or API_KEY_ID in .envrc}"
: "${LOG_API_KEY_SECRET:?Set LOG_API_KEY_SECRET or API_KEY_SECRET in .envrc}"

mkdir -p "$(dirname "$LOG_OUTPUT")"

curl -sS --get \
  --header "x-api-key: $LOG_API_KEY_ID" \
  --header "x-api-secret: $LOG_API_KEY_SECRET" \
  --data-urlencode 'source=am-everything,idm-everything' \
  --data "transactionId=${1:?Provide a transaction id}" \
  "$BASE/monitoring/logs" | jq >"$LOG_OUTPUT"

echo "Wrote transaction logs to $LOG_OUTPUT"
