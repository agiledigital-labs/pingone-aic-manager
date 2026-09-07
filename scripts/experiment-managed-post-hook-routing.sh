#!/usr/bin/env bash
# experiment-managed-post-hook-routing.sh
#
# Question this answers:
#   Does postUpdate run after a create, or only after an update?
#
# Method:
#   Install a throwaway subject type with onCreate/postCreate/onUpdate/postUpdate
#   hooks and a separate event type. Each hook creates one event record. Create
#   the subject and inspect the events, then PATCH it and inspect again.
#
# Findings this reproduces (verified 2026-09-07):
#   * create: onCreate + postCreate only
#   * update: onUpdate + postUpdate only
#
# Everything is deleted and both type definitions are removed unless KEEP=1.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
AIC="${AIC:-aic}"
SUBJECT_OBJ="${SUBJECT_OBJ:-test_post_hook_subject}"
EVENT_OBJ="${EVENT_OBJ:-test_post_hook_event}"

if ! command -v "$AIC" >/dev/null 2>&1 && [ ! -x "$AIC" ]; then
  echo "error: aic binary not found: $AIC" >&2
  exit 2
fi

if [ -z "${TENANT_BASE_URL:-}" ] && [ -f "$ROOT/.aic/config.toml" ]; then
  ctx="$($AIC --no-prompt ctx current 2>/dev/null || true)"
  TENANT_BASE_URL="$(awk -v want="$ctx" '
    function flush() { if (inblock && name == want && url != "") { print url; exit } }
    /^\[\[tenant\]\]/ { flush(); name = ""; url = ""; inblock = 1; next }
    /^\[/             { flush(); inblock = 0; next }
    inblock && /^[[:space:]]*name[[:space:]]*=/     { name = value($0) }
    inblock && /^[[:space:]]*base_url[[:space:]]*=/ { url = value($0) }
    function value(line) {
      sub(/^[^=]*=[[:space:]]*"/, "", line); sub(/".*$/, "", line); return line
    }
    END { flush() }
  ' "$ROOT/.aic/config.toml")"
fi
if [ -z "${TENANT_BASE_URL:-}" ]; then
  echo "error: could not determine TENANT_BASE_URL" >&2
  exit 2
fi
BASE="${TENANT_BASE_URL%/}"

TOKEN="${TOKEN:-$($AIC --no-prompt whoami --token 2>/dev/null || true)}"
if [ -z "$TOKEN" ]; then
  echo "error: no token from the agent; unlock it with 'aic session login'" >&2
  exit 2
fi

TMP="$(mktemp -d)"
CODE=""
FAILED=0
INSTALLED=0

say() { printf '\n== %s\n' "$1"; }

check() {
  if [ "$2" = "$3" ]; then
    printf '  ok    %-54s %s\n' "$1" "$3"
  else
    printf '  FAIL  %-54s expected %s, got %s\n' "$1" "$2" "$3"
    FAILED=$((FAILED + 1))
  fi
}

# req METHOD PATH [BODY] -> $CODE and $TMP/body
req() {
  local method="$1" path="$2" body="${3:-}"
  if [ -n "$body" ]; then
    CODE="$(curl -sS -o "$TMP/body" -w '%{http_code}' -X "$method" \
      -H "Authorization: Bearer $TOKEN" \
      -H 'Accept-API-Version: resource=1.0' \
      -H 'Content-Type: application/json' \
      -d "$body" "$BASE$path")"
  else
    CODE="$(curl -sS -o "$TMP/body" -w '%{http_code}' -X "$method" \
      -H "Authorization: Bearer $TOKEN" \
      -H 'Accept-API-Version: resource=1.0' \
      "$BASE$path")"
  fi
}

# req_file METHOD PATH FILE -> $CODE and $TMP/body
req_file() {
  CODE="$(curl -sS -o "$TMP/body" -w '%{http_code}' -X "$1" \
    -H "Authorization: Bearer $TOKEN" \
    -H 'Accept-API-Version: resource=1.0' \
    -H 'Content-Type: application/json' \
    --data-binary @"$3" "$BASE$2")"
}

events() {
  req GET "/openidm/managed/$EVENT_OBJ?_queryFilter=true&_fields=_id,phase"
  if [ "$CODE" = 200 ]; then
    jq -c '[.result[].phase] | sort' "$TMP/body"
  else
    printf 'HTTP-%s' "$CODE"
  fi
}

# Invoked by cleanup, which ShellCheck treats as an indirect trap callback.
# shellcheck disable=SC2329
remove_types() {
  req GET /openidm/config/managed
  [ "$CODE" = 200 ] || return 1
  cp "$TMP/body" "$TMP/cleanup-before.json"
  jq --arg subject "$SUBJECT_OBJ" --arg event "$EVENT_OBJ" \
    '.objects |= map(select(.name != $subject and .name != $event))' \
    "$TMP/cleanup-before.json" > "$TMP/cleanup.json"
  req_file PUT /openidm/config/managed "$TMP/cleanup.json"
  [ "$CODE" = 200 ]
}

delete_records() {
  local object="$1"
  req GET "/openidm/managed/$object?_queryFilter=true&_fields=_id"
  if [ "$CODE" = 200 ]; then
    jq -r '.result[]._id' "$TMP/body" > "$TMP/$object-ids"
    while IFS= read -r id; do
      [ -n "$id" ] && req DELETE "/openidm/managed/$object/$id"
    done < "$TMP/$object-ids"
  fi
}

# Invoked by the EXIT trap below.
# shellcheck disable=SC2329
cleanup() {
  local exit_code=$?
  trap - EXIT
  if [ -n "${KEEP:-}" ]; then
    echo
    echo "KEEP set — leaving $SUBJECT_OBJ and $EVENT_OBJ behind"
    rm -rf "$TMP"
    exit "$exit_code"
  fi

  say "cleanup"
  if [ "$INSTALLED" -eq 1 ]; then
    delete_records "$SUBJECT_OBJ"
    delete_records "$EVENT_OBJ"
    if remove_types; then
      echo "  removed records and both throwaway types"
    else
      echo "  WARNING: could not remove the throwaway types" >&2
      exit_code=1
    fi
  fi
  rm -rf "$TMP"
  exit "$exit_code"
}
trap cleanup EXIT

echo "context: $($AIC --no-prompt ctx current 2>/dev/null || echo unknown)"
echo "subject: $SUBJECT_OBJ"
echo "events:  $EVENT_OBJ"

say "install hook subject and event types"
req GET /openidm/config/managed
if [ "$CODE" != 200 ]; then
  echo "error: config/managed GET returned $CODE" >&2
  exit 2
fi
if jq -e --arg subject "$SUBJECT_OBJ" --arg event "$EVENT_OBJ" \
  '.objects[] | select(.name == $subject or .name == $event)' \
  "$TMP/body" >/dev/null; then
  echo "error: a probe type already exists; refusing to overwrite it" >&2
  exit 2
fi
cp "$TMP/body" "$TMP/managed-before.json"

jq --arg subject "$SUBJECT_OBJ" --arg event "$EVENT_OBJ" '
  def hook($event; $phase): {
    type: "text/javascript",
    source: ("openidm.create(\"managed/" + $event + "\", \"" + $phase +
      "\", { phase: \"" + $phase + "\" });")
  };
  .objects += [
    {
      name: $event,
      schema: {
        type: "object",
        title: "AIC post-hook routing probe events",
        properties: {phase: {type: "string"}},
        required: ["phase"],
        order: ["phase"]
      }
    },
    {
      name: $subject,
      schema: {
        type: "object",
        title: "AIC post-hook routing probe subject",
        properties: {value: {type: "string"}},
        required: ["value"],
        order: ["value"]
      },
      onCreate: hook($event; "onCreate"),
      postCreate: hook($event; "postCreate"),
      onUpdate: hook($event; "onUpdate"),
      postUpdate: hook($event; "postUpdate")
    }
  ]' "$TMP/managed-before.json" > "$TMP/managed-with-probe.json"

req_file PUT /openidm/config/managed "$TMP/managed-with-probe.json"
check "config write" "200" "$CODE"
[ "$CODE" = 200 ] || exit 2
INSTALLED=1

printf '  waiting for both types and hooks to become active'
ready=""
for _attempt in $(seq 1 40); do
  req GET "/openidm/managed/$SUBJECT_OBJ?_queryFilter=true&_pageSize=1"
  subject_code="$CODE"
  req GET "/openidm/managed/$EVENT_OBJ?_queryFilter=true&_pageSize=1"
  if [ "$subject_code" = 200 ] && [ "$CODE" = 200 ]; then
    ready=yes
    break
  fi
  printf '.'
  sleep 3
done
printf ' %s\n' "${ready:-gave up}"
[ -n "$ready" ] || exit 2

say "create fires only the create pair"
# Hook activation can trail type activation. If a create does not log both
# events, delete it and its partial events, then retry after a short wait.
create_ready=""
for _attempt in $(seq 1 20); do
  req PUT "/openidm/managed/$SUBJECT_OBJ/subject1" '{"value":"created"}'
  create_code="$CODE"
  observed="$(events)"
  if [ "$create_code" = 201 ] && [ "$observed" = '["onCreate","postCreate"]' ]; then
    create_ready=yes
    break
  fi
  delete_records "$SUBJECT_OBJ"
  delete_records "$EVENT_OBJ"
  sleep 3
done
check "create hook set" '["onCreate","postCreate"]' "$observed"
check "postUpdate did not run after create" "true" \
  "$(printf '%s' "$observed" | jq -r 'index("postUpdate") == null')"
[ -n "$create_ready" ] || exit 1

say "update fires only the update pair"
req PATCH "/openidm/managed/$SUBJECT_OBJ/subject1" \
  '[{"operation":"replace","field":"/value","value":"updated"}]'
check "update returns 200" "200" "$CODE"
observed="$(events)"
check "events after create then update" \
  '["onCreate","onUpdate","postCreate","postUpdate"]' "$observed"

echo
if [ "$FAILED" -eq 0 ]; then
  echo "all assertions passed"
else
  echo "$FAILED assertion(s) FAILED"
fi
exit "$FAILED"
