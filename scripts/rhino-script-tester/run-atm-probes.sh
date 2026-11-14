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
#
# The script and client are env overrides (SCRIPT_REF, WORKSPACE_CJS, CLIENT_ID).
# The defaults stay the legacy probe. A next-gen run points them at its own
# throwaway script and client. GRANT_TYPE=password also needs USERNAME and
# PASSWORD_FILE (a resource-owner grant, so `identity` is that user).
# The result records the `aud` and `zzemuR3Mctl` claim shapes. It never stores
# the access token.
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
GRANT_TYPE="${GRANT_TYPE:-client_credentials}"
USERNAME="${USERNAME:-}"
PASSWORD_FILE="${PASSWORD_FILE:-}"
FIXTURES_DIR="${FIXTURES_DIR:-$SCRIPT_DIR/fixtures-atm}"
OUT_DIR="${OUT_DIR:-$ROOT/tmp/rhino-script-tester}"

# Never hard-code the tenant host: derive it, so nothing here carries a
# client-identifying hostname into the repo.
# BASE_URL is the last column. `ctx list` gained a THEME column, so $4 is the
# theme ("sandbox") and $NF is the URL.
BASE="${BASE:-$("$AIC_BIN" ctx list --no-prompt | awk '/^\*/{print $NF}')}"
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

  if [[ "$GRANT_TYPE" != "client_credentials" && "$GRANT_TYPE" != "password" ]]; then
    echo "  unsupported GRANT_TYPE=$GRANT_TYPE" >&2
    continue
  fi
  if [[ "$GRANT_TYPE" == "password" ]]; then
    if [[ -z "$USERNAME" || ! -r "$PASSWORD_FILE" ]]; then
      echo "  password grant needs USERNAME and PASSWORD_FILE" >&2
      continue
    fi
  fi

  grant_args=(
    --data-urlencode "grant_type=$GRANT_TYPE"
    --data-urlencode "client_id=$CLIENT_ID"
    --data-urlencode "client_secret=$(cat "$SECRET_FILE")"
    --data-urlencode "scope=$SCOPE"
  )
  if [[ "$GRANT_TYPE" == "password" ]]; then
    grant_args+=(--data-urlencode "username=$USERNAME")
    grant_args+=(--data-urlencode "password=$(cat "$PASSWORD_FILE")")
  fi

  # Keep the response in the pipe; the final line carries HTTP status only.
  # The claim is the result. A 200 with no `aicprobe` means the script threw
  # before setField — AM issues the token anyway, so the status alone lies.
  { curl -sS -w '\n%{http_code}\n' \
    -X POST "$BASE/am/oauth2/realms/root/realms/$REALM/access_token" \
    "${grant_args[@]}" || printf '\n000\n'; } |
  python3 -c "$(cat <<'PY'
import base64, json, sys
results_path, name = sys.argv[1:3]
response, status = sys.stdin.read().rstrip("\n").rsplit("\n", 1)
KNOWN = {
    "zzemuR3M-aud", "zzemuR3M-a", "zzemuR3M-b",
    "zzemuR3M-ctl", "ctl-ok",
    "zzemuR3M-nostore", "zzemuR3M-stored", "zzemuR3M-baseline",
}

def shape(v, ids):
    if v is None:
        return "null"
    if isinstance(v, bool):
        return "bool:" + ("true" if v else "false")
    if isinstance(v, (int, float)):
        return "number"
    if isinstance(v, str):
        if v in KNOWN:
            return "string:" + v
        if v in ids:
            return "string:equals-client-id"
        if "://" in v:
            return "string:url:len=%d" % len(v)
        return "string:other:len=%d" % len(v)
    if isinstance(v, list):
        return "array[" + ",".join(shape(x, ids) for x in v) + "]"
    if isinstance(v, dict):
        return "object:keys=%d" % len(v)
    return type(v).__name__

try:
    body = json.loads(response)
except Exception as exc:
    body = {"_unparseable": str(exc)}
claim, note = None, ""
aud_shape, ctl_shape = None, None
tok = body.get("access_token")
if tok and tok.count(".") == 2:
    seg = tok.split(".")[1]
    seg += "=" * (-len(seg) % 4)
    try:
        payload = json.loads(base64.urlsafe_b64decode(seg))
    except Exception as exc:
        note = "jwt decode failed: %s" % exc
        payload = None
    if payload is not None:
        ids = set()
        for key in ("client_id", "sub", "azp"):
            val = payload.get(key)
            if isinstance(val, str):
                ids.add(val)
        claim = payload.get("aicprobe")
        aud_shape = shape(payload.get("aud"), ids) if "aud" in payload else "absent"
        ctl_shape = shape(payload.get("zzemuR3Mctl"), ids) if "zzemuR3Mctl" in payload else "absent"
        if claim is None and not note:
            note = ("no aicprobe claim — the fixture threw before setField, or the "
                    "client is not stateless (needs overrideOAuth2ClientConfig."
                    "statelessTokensEnabled)")
elif tok:
    note = "opaque token — set statelessTokensEnabled on the client override"
else:
    err = {k: body.get(k) for k in ("error", "error_description") if k in body}
    note = "no token: " + json.dumps(err)
rows = json.load(open(results_path))
rows.append({
    "fixture": name,
    "http": status,
    "claim": claim,
    "aud": aud_shape,
    "control": ctl_shape,
    "note": note,
})
json.dump(rows, open(results_path, "w"), indent=1)
print("  http=%s aud=%s control=%s %s" % (status, aud_shape, ctl_shape, note))
if claim is not None:
    print(json.dumps(claim, indent=2))
PY
)" "$RESULTS" "$name"
done

echo
echo "Results: $RESULTS"
