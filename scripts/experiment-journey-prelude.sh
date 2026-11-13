#!/usr/bin/env bash
# experiment-journey-prelude.sh — sourced by scripts/experiment-journey-*.sh.
#
# Resolves TENANT_BASE_URL from the aic checkout named by AIC_REPO (default:
# this checkout) and checks the agent will hand out a bearer. The bearer itself
# is fetched by experiment_journey_lib.py and never printed.
# shellcheck shell=bash

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
export AIC_REPO="${AIC_REPO:-$ROOT}"
if [ -z "${TENANT_BASE_URL:-}" ] && [ -f "$AIC_REPO/.aic/config.toml" ]; then
  ctx="$(cd "$AIC_REPO" && aic --no-prompt ctx current 2>/dev/null | head -1 || true)"
  TENANT_BASE_URL="$(awk -v want="$ctx" '
    function flush() { if (!done && inblock && name == want && url != "") { print url; done = 1; exit } }
    /^\[\[tenant\]\]/ { flush(); name = ""; url = ""; inblock = 1; next }
    /^\[/             { flush(); inblock = 0; next }
    inblock && /^[[:space:]]*name[[:space:]]*=/     { name = value($0) }
    inblock && /^[[:space:]]*base_url[[:space:]]*=/ { url = value($0) }
    function value(line) {
      sub(/^[^=]*=[[:space:]]*"/, "", line); sub(/".*$/, "", line); return line
    }
    END { flush() }
  ' "$AIC_REPO/.aic/config.toml")"
fi
if [ -z "${TENANT_BASE_URL:-}" ]; then
  echo "error: could not determine TENANT_BASE_URL (set AIC_REPO or TENANT_BASE_URL)" >&2
  exit 2
fi
export TENANT_BASE_URL
if ! (cd "$AIC_REPO" && aic --no-prompt whoami --token >/dev/null 2>&1); then
  echo "error: no bearer from the agent; run 'aic login' in $AIC_REPO" >&2
  exit 3
fi
export PYTHONPATH="$HERE${PYTHONPATH:+:$PYTHONPATH}"
# Everything a probe prints passes through the capture redactor.
REDACT="$HERE/check-sensitive-metadata.sh"
export REDACT
