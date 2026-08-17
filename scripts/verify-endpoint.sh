#!/usr/bin/env bash
# verify-endpoint.sh — curl an AIC path with a service-account bearer.
#
# Usage:
#   scripts/verify-endpoint.sh                       # check connectivity, print nothing secret
#   scripts/verify-endpoint.sh /environment/variables
#   scripts/verify-endpoint.sh /am/json/realms/root/realms/alpha/scripts?_queryFilter=true \
#       --header "Accept-API-Version: protocol=2.0,resource=1.0"
#
# Tenant base URL, in order of precedence:
#   1. $TENANT_BASE_URL from the environment
#   2. `base_url` for the active context in .aic/config.toml  <- the usual case
#   3. .envrc, sourced only if it happens to export TENANT_BASE_URL
#
# (2) is where the URL actually lives now. It used to be an `export` in .envrc;
# onboarding moved it into the project config, and until 2026-08-17 this script
# only looked at .envrc — so it failed with "TENANT_BASE_URL is not set (check
# .envrc)" on a correctly configured project, and sourcing .envrc under a
# non-direnv shell also spat `use: command not found` from its `use nix` line.
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

# --- Tenant base URL ---
# The project config is the normal source. Read the `base_url` of the active
# context rather than the first one in the file, so a multi-tenant config does
# not silently curl the wrong tenant with this tenant's bearer.
if [ -z "${TENANT_BASE_URL:-}" ] && [ -f "$ROOT/.aic/config.toml" ]; then
  ctx=$(aic --no-prompt ctx current 2>/dev/null || true)
  # Config is an array of tables: a `[[tenant]]` block per tenant, each with a
  # `name` and a `base_url` in no guaranteed order. So buffer both per block and
  # emit only when the block ends and the name matched.
  TENANT_BASE_URL=$(awk -v want="$ctx" '
    function flush() { if (inblock && name == want && url != "") { print url; exit } }
    /^\[\[tenant\]\]/ { flush(); name = ""; url = ""; inblock = 1; next }
    /^\[/             { flush(); inblock = 0; next }
    inblock && /^[[:space:]]*name[[:space:]]*=/     { name = value($0) }
    inblock && /^[[:space:]]*base_url[[:space:]]*=/ { url  = value($0) }
    function value(line) {
      sub(/^[^=]*=[[:space:]]*"/, "", line); sub(/".*$/, "", line); return line
    }
    END { flush() }
  ' "$ROOT/.aic/config.toml")
fi

# Last resort: .envrc, for a checkout that still exports it. Sourced in a
# subshell-safe way — it may contain direnv-only commands (`use nix`) that are
# not valid bash, and it holds secrets we must not let fail the script.
if [ -z "${TENANT_BASE_URL:-}" ] && [ -f "$ROOT/.envrc" ]; then
  TENANT_BASE_URL=$(
    set -a
    # shellcheck disable=SC1091
    source "$ROOT/.envrc" 2>/dev/null || true
    set +a
    printf '%s' "${TENANT_BASE_URL:-}"
  )
fi

if [ -z "${TENANT_BASE_URL:-}" ]; then
  echo "error: could not determine the tenant base URL." >&2
  echo "  looked for: \$TENANT_BASE_URL, base_url for the active context in" >&2
  echo "  .aic/config.toml, then .envrc. Check 'aic ctx current'." >&2
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
