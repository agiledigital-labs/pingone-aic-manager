#!/usr/bin/env bash
# experiment-managed-defaults.sh
#
# Question this answers:
#   Is a managed-property `default` actually applied by IDM, or is it only
#   metadata the admin console reads when it prefills a form? And how does it
#   interact with `required` — does a required property with a default still
#   demand a value from every caller?
#
# Method:
#   Build a throwaway managed object through `aic managed field add --default`
#   (so the CLI's own coercion is what writes the schema), then create a record
#   that omits every defaulted property and read it back. One field defaults to
#   `true` and one to a non-empty array, so an applied default is distinguishable
#   from an absent property — a probe using only `false`/`0`/`[]` cannot tell
#   "server applied the default" from "property missing, JSON null-ish".
#
#   Then exercise the write paths that look like they should be equivalent and
#   are not: explicit `null`, whole-record PUT with the property omitted, and
#   `PATCH remove`.
#
# Findings this reproduces (verified 2026-08-05, docs/api/10-managed-objects.md):
#   * `default` IS server-applied, on create, for string / number / boolean /
#     string[] — including `0` and `[]`. For an array it lives on the outer
#     property, beside `items`.
#   * It is applied BEFORE policy validation, so a `required` property carrying a
#     default creates cleanly when the caller omits it entirely.
#   * An explicit `null` is refused NOT_NULL whether or not the property is
#     required — so removing `required` does not rescue a null-sending caller.
#     Omit the key instead.
#   * `required` does NOT protect a whole-record PUT: omitting the property on
#     update returns 200 and erases the stored value. Only `PATCH remove` is
#     refused. Defaults apply on create only, so nothing puts the value back.
#
# Token: taken from the running `aic agent` ($AIC whoami --token). No pyjwt and
#   no verify-endpoint.sh needed. The agent must be unlocked.
#
# Usage:
#   scripts/experiment-managed-defaults.sh
#   AIC=/path/to/aic scripts/experiment-managed-defaults.sh
#   MISMATCH=1 scripts/experiment-managed-defaults.sh   # + the bricking case
#   KEEP=1 scripts/experiment-managed-defaults.sh       # leave the object behind
#
# Env:
#   AIC        aic binary                     (default ./target/debug/aic)
#   OBJ        throwaway object name          (default test_defaults_probe)
#   MISMATCH   also prove a type-mismatched default bricks the object. Off by
#              default because it writes config the CLI deliberately refuses,
#              and the object stays 404 for minutes before and after the repair.
#   KEEP       skip cleanup
#
# Exit status is the number of failed assertions, so this is usable as a check
# and not just a report.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
AIC="${AIC:-$ROOT/target/debug/aic}"
OBJ="${OBJ:-test_defaults_probe}"

if [ -z "${TENANT_BASE_URL:-}" ] && [ -f "$ROOT/.envrc" ]; then
  TENANT_BASE_URL="$(grep -oE 'https://[^"]+forgeblocks\.com' "$ROOT/.envrc" | head -1 || true)"
fi
if [ -z "${TENANT_BASE_URL:-}" ]; then
  echo "error: could not determine TENANT_BASE_URL (set it or check .envrc)" >&2
  exit 2
fi
BASE="${TENANT_BASE_URL%/}"

if [ ! -x "$AIC" ]; then
  echo "error: $AIC not found — build it (cargo build) or set AIC=" >&2
  exit 2
fi
TOKEN=${TOKEN:-"$("$AIC" whoami --token 2>/dev/null || true)"}
if [ -z "$TOKEN" ]; then
  echo "error: no token from the agent. Is it running and unlocked? ($AIC status)" >&2
  exit 2
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
FAILED=0

say() { printf '\n== %s\n' "$1"; }

# check LABEL EXPECTED ACTUAL
check() {
  if [ "$2" = "$3" ]; then
    printf '  ok    %-52s %s\n' "$1" "$3"
  else
    printf '  FAIL  %-52s expected %s, got %s\n' "$1" "$2" "$3"
    FAILED=$((FAILED + 1))
  fi
}

# req_file METHOD PATH FILE -> sets $CODE, body in $TMP/body
#
# The whole `config/managed` document is megabytes, which is well past ARG_MAX —
# passing it as a `-d` argument fails with "Argument list too long". Anything
# that sends the full config has to stream it from a file.
req_file() {
  CODE="$(curl -sS -o "$TMP/body" -w '%{http_code}' -X "$1" \
    -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
    --data-binary @"$3" "$BASE$2")"
}

# req METHOD PATH [BODY] -> sets $CODE, body in $TMP/body
req() {
  local method="$1" path="$2" body="${3:-}"
  if [ -n "$body" ]; then
    CODE="$(curl -sS -o "$TMP/body" -w '%{http_code}' -X "$method" \
      -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
      -d "$body" "$BASE$path")"
  else
    CODE="$(curl -sS -o "$TMP/body" -w '%{http_code}' -X "$method" \
      -H "Authorization: Bearer $TOKEN" "$BASE$path")"
  fi
}

field() { # field KEY TYPE [extra flags...]
  local key="$1" type="$2"; shift 2
  "$AIC" managed field add "$OBJ.$key" --type "$type" --yes "$@" >/dev/null \
    || { echo "  FAIL  could not add $key" >&2; FAILED=$((FAILED + 1)); }
}

cleanup() {
  [ -n "${KEEP:-}" ] && { echo; echo "KEEP set — leaving $OBJ behind"; return; }
  say "cleanup"
  req GET "/openidm/managed/$OBJ?_queryFilter=true&_fields=_id"
  if [ "$CODE" = 200 ]; then
    for id in $(jq -r '.result[]._id' "$TMP/body" 2>/dev/null); do
      req DELETE "/openidm/managed/$OBJ/$id"
    done
  fi
  "$AIC" managed object delete "$OBJ" --yes >/dev/null 2>&1 \
    && echo "  removed $OBJ" || echo "  WARNING: could not remove $OBJ"
}

echo "tenant: $BASE"
echo "aic:    $AIC"
echo "object: $OBJ"

# ---------------------------------------------------------------- build schema
say "build $OBJ through the CLI"
"$AIC" managed object delete "$OBJ" --yes >/dev/null 2>&1 || true
"$AIC" managed object create "$OBJ" --title "Managed default probe" --yes >/dev/null \
  || { echo "error: could not create $OBJ" >&2; exit 2; }

field control "string"                                    # no default: the control
field sDef    "string"   --default hello
field nDef    "number"   --default 7
field nZero   "number"   --default 0
field bTrue   "boolean"  --default true                   # discriminating: not falsy
field bReq    "boolean"  --default false --required true   # default vs required
field aDef    "string[]" --default '["a","b"]'
field aEmpty  "string[]" --default '[]'

say "stored schema"
"$AIC" managed get "$OBJ" | jq -c '.schema.properties | with_entries(select(.value.default != null)) | with_entries(.value |= {type, default})'
check "array default sits beside items, not inside" "true" \
  "$("$AIC" managed get "$OBJ" | jq -c '(.schema.properties.aDef.default != null) and (.schema.properties.aDef.items.default == null)')"
check "bReq is in schema.required" "true" \
  "$("$AIC" managed get "$OBJ" | jq -c '.schema.required | index("bReq") != null')"

# The managed type is instantiated asynchronously after the config write.
printf '\n== waiting for the type to answer queries'
for _ in $(seq 1 40); do
  req GET "/openidm/managed/$OBJ?_queryFilter=true&_pageSize=1"
  [ "$CODE" = 200 ] && break
  printf '.'; sleep 3
done
printf ' %s\n' "$CODE"
if [ "$CODE" != 200 ]; then
  echo "  FAIL  type never came live; aborting" >&2
  cleanup
  exit $((FAILED + 1))
fi

# ...but answering queries is NOT a signal that the *property* schema is
# effective. Observed 2026-08-05: immediately after the type started serving
# 200s, creates succeeded while applying no defaults at all and enforcing no
# policy — an explicit null on a required property returned 201. Seconds later
# the same calls behaved correctly. This is the record-policy lag already noted
# in docs/api/10-managed-objects.md ("schema changes are not immediately
# effective for record policy"), and it applies to defaults too.
#
# So the only trustworthy readiness signal is a default actually landing.
printf '== waiting for defaults to become effective'
warm=""
for i in $(seq 1 30); do
  req PUT "/openidm/managed/$OBJ/warmup$i" '{"control":"warmup"}'
  if [ "$CODE" = 201 ] && [ "$(jq -r '.sDef' "$TMP/body")" = "hello" ]; then
    req DELETE "/openidm/managed/$OBJ/warmup$i"
    warm=yes
    break
  fi
  [ "$CODE" = 201 ] && req DELETE "/openidm/managed/$OBJ/warmup$i"
  printf '.'; sleep 4
done
printf ' %s\n' "${warm:-gave up}"
if [ -z "$warm" ]; then
  echo "  FAIL  defaults never became effective; aborting" >&2
  cleanup
  exit $((FAILED + 1))
fi

# ------------------------------------------------- defaults applied on create?
say "create a record that omits every defaulted property"
req PUT "/openidm/managed/$OBJ/probe1" '{"control":"set by hand"}'
check "create returns 201" "201" "$CODE"
jq -c 'del(._rev)' "$TMP/body"

req GET "/openidm/managed/$OBJ/probe1"
read_back() { jq -r "$1" "$TMP/body"; }
check "string default applied"          "hello"   "$(read_back '.sDef')"
check "number default applied"          "7"       "$(read_back '.nDef')"
check "number default 0 applied"        "0"       "$(read_back '.nZero')"
check "boolean default true applied"    "true"    "$(read_back '.bTrue')"
check "required+default needs no value" "false"   "$(read_back '.bReq')"
check "array default applied"           '["a","b"]' "$(jq -c '.aDef' "$TMP/body")"
check "empty-array default applied"     '[]'      "$(jq -c '.aEmpty' "$TMP/body")"
check "control field stays absent"      "true"    "$(jq -c '.control == "set by hand"' "$TMP/body")"
check "no default means no key"         "true"    "$(jq -c 'has("nope") == false' "$TMP/body")"

# ------------------------------------------------------- null is not "unset"
say "an explicit null is not the same as omitting the key"
req PUT "/openidm/managed/$OBJ/probe2" '{"control":"x","bReq":null}'
check "explicit null on a required+default field" "403" "$CODE"
check "  refused NOT_NULL" "true" \
  "$(jq -c '[.detail.failedPolicyRequirements[].policyRequirements[].policyRequirement] | index("NOT_NULL") != null' "$TMP/body")"

# Same again with `required` dropped: the null guard is independent of it.
"$AIC" managed field edit "$OBJ.bReq" --required false --yes >/dev/null
sleep 8
req PUT "/openidm/managed/$OBJ/probe3" '{"control":"x","bReq":null}'
check "explicit null once bReq is optional" "403" "$CODE"
req PUT "/openidm/managed/$OBJ/probe4" '{"control":"x"}'
check "omitting it instead still defaults" "false" "$(jq -r '.bReq' "$TMP/body")"
"$AIC" managed field edit "$OBJ.bReq" --required true --yes >/dev/null
sleep 8

# ------------------------------- update paths: defaults are a create-time thing
say "update paths — required does not protect a whole-record PUT"
req PUT "/openidm/managed/$OBJ/probe1" '{"control":"rewritten"}'
check "whole-record PUT omitting the property" "200" "$CODE"
check "  the stored value was ERASED, not re-defaulted" "true" \
  "$(jq -c 'has("bReq") == false and has("sDef") == false' "$TMP/body")"

req PATCH "/openidm/managed/$OBJ/probe4" '[{"operation":"remove","field":"/bReq"}]'
check "PATCH remove on a required property" "403" "$CODE"
req PATCH "/openidm/managed/$OBJ/probe4" '[{"operation":"replace","field":"/bReq","value":null}]'
check "PATCH replace with null" "400" "$CODE"

# ------------------------------------------- optional: the bricking case
if [ -n "${MISMATCH:-}" ]; then
  say "a type-mismatched default bricks the object (config PUT says 200)"
  # Deliberately bypasses the CLI: `aic managed field add --default` coerces
  # against the declared type and refuses this, which is the whole point of the
  # local validation. Only a raw config write can produce the broken shape.
  req GET /openidm/config/managed
  jq --arg o "$OBJ" \
    '(.objects[] | select(.name==$o) | .schema.properties.bTrue.default) |= "not-a-boolean"' \
    "$TMP/body" > "$TMP/bad.json"
  req_file PUT /openidm/config/managed "$TMP/bad.json"
  check "config PUT accepts the mismatch" "200" "$CODE"
  printf '  polling the object for 60s'
  live=""
  for _ in $(seq 1 20); do
    req GET "/openidm/managed/$OBJ?_queryFilter=true&_pageSize=1"
    [ "$CODE" = 200 ] && { live=yes; break; }
    printf '.'; sleep 3
  done
  printf '\n'
  check "object is now unreachable" "" "$live"
  # Blast radius: a healthy neighbour must still serve.
  req GET "/openidm/managed/alpha_user?_queryFilter=true&_pageSize=1"
  check "a neighbouring type still serves" "200" "$CODE"
  # Repair.
  req GET /openidm/config/managed
  jq --arg o "$OBJ" \
    '(.objects[] | select(.name==$o) | .schema.properties.bTrue.default) |= true' \
    "$TMP/body" > "$TMP/fixed.json"
  req_file PUT /openidm/config/managed "$TMP/fixed.json"
  check "repair PUT" "200" "$CODE"
fi

cleanup

echo
if [ "$FAILED" -eq 0 ]; then
  echo "all assertions passed"
else
  echo "$FAILED assertion(s) FAILED"
fi
exit "$FAILED"
