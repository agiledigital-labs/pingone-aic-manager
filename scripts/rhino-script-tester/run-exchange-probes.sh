#!/usr/bin/env bash
# Probe runner for RFC 8693 **token exchange** (GAPS D3 and D6).
#
# The OAuth2 lane next door drives one client on `client_credentials`. An
# exchange needs two: a SUBJECT client whose tokens carry `may_act`, and an
# ACTOR client that holds the token-exchange grant and presents the subject's
# token. So this lane is a third harness rather than a flag on the second.
#
# It runs in **bravo**, and only bravo. The sandbox's `alpha` realm configures
# the same four `tokenExchangeClasses` but does not list the grant in
# `advancedOAuth2Config.grantTypes`, so every exchange there fails
# `unsupported_grant_type` before any script runs — which would make every case
# below look identical and prove nothing. `aic oauth exchange list --realm
# <realm>` answers this before you run anything.
#
# The experiment, and what each case is a control for:
#
#   control-cc         may-act stamps | identity probe | client_credentials
#                      Positive control. Proves the override wiring and the
#                      entry point work, and shows what `identity` is on a
#                      grant that is NOT an exchange. Without this, "identity
#                      is empty on the exchange" could just mean the script
#                      never ran.
#   control-exchange   may-act stamps | passthrough    | token-exchange
#                      Proves the exchange itself works with this pair of
#                      clients, independently of what the identity probe finds.
#   identity           may-act stamps | identity probe | token-exchange
#                      D3. Compare its log line against control-cc's.
#   no-may-act         may-act SILENT | passthrough    | token-exchange
#                      D6. The discriminating case: same clients, same scopes,
#                      same script — only the `setMayAct` call is gone. If this
#                      still issues a token, `may_act` is not the gate.
#
# Usage:
#   SUBJECT_SECRET_FILE=... ACTOR_SECRET_FILE=... \
#     scripts/rhino-script-tester/run-exchange-probes.sh [case ...]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

TENANT="${TENANT:-sandbox}"
REALM="${REALM:-bravo}"
AIC_BIN="${AIC_BIN:-$ROOT/target/debug/aic}"
SUBJECT_CLIENT="${SUBJECT_CLIENT:-aic-probe-xchg-subject}"
ACTOR_CLIENT="${ACTOR_CLIENT:-aic-probe-xchg-actor}"
MAYACT_REF="${MAYACT_REF:-$REALM/AIC-MayAct-Probe}"
VALIDATE_REF="${VALIDATE_REF:-$REALM/AIC-XchgScope-Probe}"
MAYACT_CJS="${MAYACT_CJS:-$ROOT/workspace/$TENANT/am/$REALM/oauth2-may-act-ng/AIC-MayAct-Probe.cjs}"
VALIDATE_CJS="${VALIDATE_CJS:-$ROOT/workspace/$TENANT/am/$REALM/oauth2-validate-scope-ng/AIC-XchgScope-Probe.cjs}"
# Two scopes for the same reason the OAuth2 lane uses two: a single-scope
# request hides a partial narrowing behind a clean 403.
SCOPE="${SCOPE:-aicedit-probe aicedit-probe-keep}"
FIXTURES_DIR="${FIXTURES_DIR:-$SCRIPT_DIR/fixtures-exchange}"
OUT_DIR="${OUT_DIR:-$ROOT/tmp/rhino-script-tester}"

# Never hard-code the tenant host: derive it, so nothing here carries a
# client-identifying hostname into the repo.
BASE="${BASE:-$("$AIC_BIN" ctx list --no-prompt | awk '/^\*/{print $4}')}"
[[ -n "$BASE" ]] || { echo "Could not resolve tenant base URL" >&2; exit 1; }

for var in SUBJECT_SECRET_FILE ACTOR_SECRET_FILE; do
  path="${!var:-}"
  [[ -n "$path" && -r "$path" ]] || {
    echo "Set $var to a readable file holding that client's secret" >&2; exit 1; }
done
SUBJECT_SECRET="$(cat "$SUBJECT_SECRET_FILE")"
ACTOR_SECRET="$(cat "$ACTOR_SECRET_FILE")"

need() { command -v "$1" >/dev/null 2>&1 || { echo "Missing: $1" >&2; exit 1; }; }
need curl; need jq

mkdir -p "$OUT_DIR"
RESULTS="$OUT_DIR/exchange-probe-results.json"
TOKEN_URL="$BASE/am/oauth2/realms/root/realms/$REALM/access_token"
EXCHANGE_GRANT="urn:ietf:params:oauth:grant-type:token-exchange"

# case = name|mayact fixture|validate fixture|grant
CASES=(
  "control-cc|mayact-stamp|identity-probe|client_credentials"
  "control-exchange|mayact-stamp|control-passthrough|exchange"
  "identity|mayact-stamp|identity-probe|exchange"
  "no-may-act|mayact-silent|control-passthrough|exchange"
)

if [[ $# -gt 0 ]]; then
  wanted=("$@")
  selected=()
  for want in "${wanted[@]}"; do
    for row in "${CASES[@]}"; do
      [[ "${row%%|*}" == "$want" ]] && selected+=("$row")
    done
  done
  [[ ${#selected[@]} -gt 0 ]] || { echo "No such case: ${wanted[*]}" >&2; exit 1; }
  CASES=("${selected[@]}")
fi

tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
echo "[]" > "$RESULTS"

push_fixture() {  # push_fixture <fixture file> <workspace cjs> <script ref>
  local fixture="$1" dest="$2" ref="$3"
  # `AIC_ACTOR_CLIENT_ID` is a placeholder rather than the real id so the
  # committed fixture carries no tenant-specific value.
  sed "s/AIC_ACTOR_CLIENT_ID/$ACTOR_CLIENT/g" "$fixture" > "$dest"
  "$AIC_BIN" script push "$ref" --no-prompt
}

subject_token() {  # -> stdout: the access token, or empty
  curl -sS -X POST "$TOKEN_URL" \
    --data-urlencode "grant_type=client_credentials" \
    --data-urlencode "client_id=$SUBJECT_CLIENT" \
    --data-urlencode "client_secret=$SUBJECT_SECRET" \
    --data-urlencode "scope=$SCOPE" \
  | jq -r '.access_token // empty'
}

for row in "${CASES[@]}"; do
  IFS='|' read -r name mayact validate grant <<< "$row"
  printf '=== %s (mayact=%s validate=%s grant=%s)\n' "$name" "$mayact" "$validate" "$grant"

  push_fixture "$FIXTURES_DIR/$mayact.mayact.js" "$MAYACT_CJS" "$MAYACT_REF" >"$tmp/push.log" 2>&1 \
    || { echo "  may-act push FAILED"; sed 's/^/    /' "$tmp/push.log"; continue; }
  push_fixture "$FIXTURES_DIR/$validate.validate.js" "$VALIDATE_CJS" "$VALIDATE_REF" >>"$tmp/push.log" 2>&1 \
    || { echo "  validate push FAILED"; sed 's/^/    /' "$tmp/push.log"; continue; }

  # The script body is read per token request, but give AM a moment to settle.
  sleep 2

  if [[ "$grant" == "exchange" ]]; then
    subject="$(subject_token)"
    if [[ -z "$subject" ]]; then
      echo "  subject token FAILED — the exchange cannot be attempted"
      continue
    fi
    status="$(curl -sS -o "$tmp/tok.json" -D "$tmp/tok.hdr" -w '%{http_code}' \
      -X POST "$TOKEN_URL" \
      --data-urlencode "grant_type=$EXCHANGE_GRANT" \
      --data-urlencode "client_id=$ACTOR_CLIENT" \
      --data-urlencode "client_secret=$ACTOR_SECRET" \
      --data-urlencode "subject_token=$subject" \
      --data-urlencode "subject_token_type=urn:ietf:params:oauth:token-type:access_token" \
      --data-urlencode "requested_token_type=urn:ietf:params:oauth:token-type:access_token" \
      --data-urlencode "scope=$SCOPE")"
  else
    status="$(curl -sS -o "$tmp/tok.json" -D "$tmp/tok.hdr" -w '%{http_code}' \
      -X POST "$TOKEN_URL" \
      --data-urlencode "grant_type=client_credentials" \
      --data-urlencode "client_id=$SUBJECT_CLIENT" \
      --data-urlencode "client_secret=$SUBJECT_SECRET" \
      --data-urlencode "scope=$SCOPE")"
  fi

  txid="$(awk -F': ' 'tolower($1)=="x-forgerock-transactionid"{gsub(/\r/,"",$2);print $2}' "$tmp/tok.hdr")"
  granted="$(jq -r 'if has("scope") then (.scope|tostring) else "<no scope field>" end' "$tmp/tok.json" 2>/dev/null || echo "<unparseable>")"
  err="$(jq -r '[.error // empty, .error_description // empty] | map(select(. != "")) | join(": ")' "$tmp/tok.json" 2>/dev/null || echo "")"
  issued="$(jq -r 'if has("access_token") then "yes" else "no" end' "$tmp/tok.json" 2>/dev/null || echo "no")"

  printf '  http=%s token_issued=%s scope=%s %s\n' "$status" "$issued" "$granted" "${err:+error=$err}"

  jq --arg c "$name" --arg m "$mayact" --arg v "$validate" --arg g "$grant" \
     --arg s "$status" --arg i "$issued" --arg sc "$granted" --arg e "$err" --arg t "$txid" \
     '. += [{case:$c, mayact:$m, validate:$v, grant:$g, http:$s, token_issued:$i, scope:$sc, error:$e, txid:$t}]' \
     "$RESULTS" > "$RESULTS.tmp" && mv "$RESULTS.tmp" "$RESULTS"
done

echo
echo "Results: $RESULTS"
jq -r '.[] | "\(.http)  issued=\(.token_issued)  scope=\(.scope)  \(.case)"' "$RESULTS"
echo
echo "The identity answer is in the logs, not the response:"
echo "  aic logs tx \"\$(jq -r '.[] | select(.case==\"identity\") | .txid' $RESULTS)\" | jq -r '.[] | .payload.message? // empty' | grep AICEDIT-D3"
