#!/usr/bin/env bash
# verify-endpoint.sh — curl an AIC path with a service-account bearer.
#
# Usage:
#   scripts/verify-endpoint.sh                       # check connectivity, print nothing secret
#   scripts/verify-endpoint.sh /environment/variables
#   scripts/verify-endpoint.sh /am/json/realms/root/realms/alpha/scripts?_queryFilter=true \
#       --header "Accept-API-Version: protocol=2.0,resource=1.0"
#
# Env (loaded from .envrc if direnv is not active):
#   TENANT_BASE_URL
#
# The token comes from the running agent via `aic whoami --token`, for the
# tenant in the current context (`aic ctx current`). The agent must be unlocked;
# `aic login` does that. This script used to sign its own assertion from a JWK
# in .envrc and cache the result in .token-cache — that stopped working when
# credentials moved into the encrypted vault, and it is no longer the right
# design anyway: the agent already holds a bearer in memory and refreshes it, so
# there is nothing to re-derive and no reason to put a token on disk.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"

# --- Load env if not already set ---
if [ -z "${TENANT_BASE_URL:-}" ] && [ -f "$ROOT/.envrc" ]; then
  set -a
  # shellcheck disable=SC1091
  source "$ROOT/.envrc"
  set +a
fi

if [ -z "${TENANT_BASE_URL:-}" ]; then
  echo "error: TENANT_BASE_URL is not set (check .envrc)" >&2
  exit 2
fi

# --- Token from the agent ---
# --no-prompt so a locked agent fails fast (exit 3) instead of waiting on a
# password prompt that an automated caller cannot answer.
if ! TOKEN=$(aic --no-prompt whoami --token 2>/dev/null) || [ -z "$TOKEN" ]; then
  echo "error: could not get a token from the agent." >&2
  echo "  the agent is probably locked — run: aic login" >&2
  echo "  check which tenant is active with: aic ctx current" >&2
  exit 3
fi
TOKEN_URL="agent (aic whoami --token)"

# --- If no path given, just confirm we can authenticate ---
if [ "$#" -eq 0 ]; then
  echo "ok: got a bearer for $(aic --no-prompt ctx current 2>/dev/null || echo 'current context')" >&2
  exit 0
fi

PATH_ARG="$1"
shift

# --- Default Accept-API-Version unless caller provided one ---
have_apiver=0
for a in "$@"; do
  case "$a" in
    *"Accept-API-Version"*) have_apiver=1 ;;
  esac
done

extra_headers=()
if [ "$have_apiver" -eq 0 ]; then
  extra_headers+=(--header "Accept-API-Version: resource=1.0")
fi

URL="${TENANT_BASE_URL%/}${PATH_ARG}"
echo "GET $URL" >&2
echo "  token from: $TOKEN_URL" >&2

curl -sS -o /tmp/aic-verify.body -w "HTTP %{http_code}\n" \
  --header "Authorization: Bearer $TOKEN" \
  --header "Accept: application/json" \
  "${extra_headers[@]}" \
  "$@" \
  "$URL" >&2

if jq -e . /tmp/aic-verify.body >/dev/null 2>&1; then
  jq . /tmp/aic-verify.body
else
  cat /tmp/aic-verify.body
fi
