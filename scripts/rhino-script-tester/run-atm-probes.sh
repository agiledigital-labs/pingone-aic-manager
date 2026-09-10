#!/usr/bin/env bash
# Probe runner for the LEGACY access-token-modification context
# (`OAUTH2_ACCESS_TOKEN_MODIFICATION`, evaluatorVersion 1.0).
#
# The journey harness cannot reach this context and neither can the OAuth2 lane
# next door: that one drives the next-gen validate-scope script, which binds a
# next-gen `Identity`. A classic `AMIdentity` — the one with `getAttribute` —
# only appears here and in legacy OIDC claims, which is why this lane exists.
#
# The return channel is a TOKEN CLAIM, not a log line: the fixture calls
# `accessToken.setField("aicprobe", …)` and the client reads it back out of the
# stateless JWT. That is faster and more reliable than log ingestion (~60s, and
# GAPS T11), and it needs no log API keys.
#
# Usage:
#   SECRET_FILE=/path/to/secret scripts/rhino-script-tester/run-atm-probes.sh [fixture.js ...]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

TENANT="${TENANT:-sandbox}"
REALM="${REALM:-alpha}"
AIC_BIN="${AIC_BIN:-$ROOT/target/debug/aic}"
CLIENT_ID="${CLIENT_ID:-aicatm-probe2}"
SCRIPT_REF="${SCRIPT_REF:-$REALM/AIC ATM Legacy Probe}"
WORKSPACE_CJS="${WORKSPACE_CJS:-$ROOT/workspace/$TENANT/am/$REALM/oauth2-access-token/AIC ATM Legacy Probe.cjs}"
SCOPE="${SCOPE:-aicedit-probe}"
FIXTURES_DIR="${FIXTURES_DIR:-$SCRIPT_DIR/fixtures-atm}"
OUT_DIR="${OUT_DIR:-$ROOT/tmp/rhino-script-tester}"

# Never hard-code the tenant host: derive it, so nothing here carries a
# client-identifying hostname into the repo.
BASE="${BASE:-$("$AIC_BIN" ctx list --no-prompt | awk '/^\*/{print $4}')}"
[[ -n "$BASE" ]] || { echo "Could not resolve tenant base URL" >&2; exit 1; }

SECRET_FILE="${SECRET_FILE:-}"
[[ -n "$SECRET_FILE" && -r "$SECRET_FILE" ]] || {
  echo "Set SECRET_FILE to a file holding the probe client's secret" >&2; exit 1; }

need() { command -v "$1" >/dev/null 2>&1 || { echo "Missing: $1" >&2; exit 1; }; }
need curl; need python3

mkdir -p "$OUT_DIR"
RESULTS="$OUT_DIR/atm-probe-results.json"

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

  status="$(curl -sS -o "$tmp/tok.json" -w '%{http_code}' \
    -X POST "$BASE/am/oauth2/realms/root/realms/$REALM/access_token" \
    --data-urlencode "grant_type=client_credentials" \
    --data-urlencode "client_id=$CLIENT_ID" \
    --data-urlencode "client_secret=$(cat "$SECRET_FILE")" \
    --data-urlencode "scope=$SCOPE")"

  # The claim is the result. A 200 with no `aicprobe` means the script threw
  # before setField — AM issues the token anyway, so the status alone lies.
  python3 - "$tmp/tok.json" "$RESULTS" "$name" "$status" <<'PY'
import base64, json, sys
tok_path, results_path, name, status = sys.argv[1:5]
try:
    body = json.load(open(tok_path))
except Exception as exc:
    body = {"_unparseable": str(exc)}
claim, note = None, ""
tok = body.get("access_token")
if tok and tok.count(".") == 2:
    seg = tok.split(".")[1]
    seg += "=" * (-len(seg) % 4)
    try:
        claim = json.loads(base64.urlsafe_b64decode(seg)).get("aicprobe")
    except Exception as exc:
        note = f"jwt decode failed: {exc}"
    if claim is None and not note:
        note = "no aicprobe claim — the fixture threw before setField, or the "\
               "client is not stateless (needs overrideOAuth2ClientConfig."\
               "statelessTokensEnabled)"
elif tok:
    note = "opaque token — set statelessTokensEnabled on the client override"
else:
    note = "no token: " + json.dumps({k: body.get(k) for k in ("error", "error_description")})
rows = json.load(open(results_path))
rows.append({"fixture": name, "http": status, "claim": claim, "note": note})
json.dump(rows, open(results_path, "w"), indent=1)
print(f"  http={status} {note}")
if claim is not None:
    print(json.dumps(claim, indent=2))
PY
done

echo
echo "Results: $RESULTS"
