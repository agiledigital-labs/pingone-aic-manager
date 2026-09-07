#!/usr/bin/env bash
# experiment-managed-hook-relationships.sh
#
# Question this answers:
#   Does the `object` binding in a managed-object onUpdate hook contain
#   relationship properties? Does `returnByDefault` change that? Can the hook
#   reliably obtain them with an explicit `openidm.read` projection?
#
# Method:
#   Install two throwaway custom managed-object types in one config write. The
#   source type has two otherwise-identical has-one relationships
#   (`returnByDefault: false` versus true) and an onUpdate hook.
#   The hook records which relationships it saw on `object`/`oldObject`, then
#   performs an explicit projected read and records those results too.
#
#   Create a target and source record, then PATCH an unrelated scalar. Everything
#   is deleted and the two type definitions are removed unless KEEP=1.
#
# Usage:
#   scripts/experiment-managed-hook-relationships.sh
#   AIC=/path/to/aic scripts/experiment-managed-hook-relationships.sh
#   KEEP=1 scripts/experiment-managed-hook-relationships.sh

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
AIC="${AIC:-aic}"
SOURCE_OBJ="${SOURCE_OBJ:-test_hook_relationship_probe}"
TARGET_OBJ="${TARGET_OBJ:-test_hook_relationship_target}"

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
    printf '  ok    %-58s %s\n' "$1" "$3"
  else
    printf '  FAIL  %-58s expected %s, got %s\n' "$1" "$2" "$3"
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

remove_types() {
  req GET /openidm/config/managed
  [ "$CODE" = 200 ] || return 1
  cp "$TMP/body" "$TMP/cleanup-before.json"
  jq --arg source "$SOURCE_OBJ" --arg target "$TARGET_OBJ" \
    '.objects |= map(select(.name != $source and .name != $target))' \
    "$TMP/cleanup-before.json" > "$TMP/cleanup.json"
  if cmp -s "$TMP/cleanup-before.json" "$TMP/cleanup.json"; then
    return 0
  fi
  req_file PUT /openidm/config/managed "$TMP/cleanup.json"
  [ "$CODE" = 200 ]
}

cleanup() {
  local exit_code=$?
  trap - EXIT
  if [ -n "${KEEP:-}" ]; then
    echo
    echo "KEEP set — leaving $SOURCE_OBJ and $TARGET_OBJ behind"
    rm -rf "$TMP"
    exit "$exit_code"
  fi

  say "cleanup"
  if [ "$INSTALLED" -eq 1 ]; then
    req GET "/openidm/managed/$SOURCE_OBJ?_queryFilter=true&_fields=_id"
    if [ "$CODE" = 200 ]; then
      while IFS= read -r id; do
        [ -n "$id" ] && req DELETE "/openidm/managed/$SOURCE_OBJ/$id"
      done < <(jq -r '.result[]._id' "$TMP/body" 2>/dev/null)
    fi
    req GET "/openidm/managed/$TARGET_OBJ?_queryFilter=true&_fields=_id"
    if [ "$CODE" = 200 ]; then
      while IFS= read -r id; do
        [ -n "$id" ] && req DELETE "/openidm/managed/$TARGET_OBJ/$id"
      done < <(jq -r '.result[]._id' "$TMP/body" 2>/dev/null)
    fi
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
echo "source: $SOURCE_OBJ"
echo "target: $TARGET_OBJ"

say "install two throwaway custom managed-object types"
req GET /openidm/config/managed
if [ "$CODE" != 200 ]; then
  echo "error: config/managed GET returned $CODE" >&2
  exit 2
fi
if jq -e --arg source "$SOURCE_OBJ" --arg target "$TARGET_OBJ" \
  '.objects[] | select(.name == $source or .name == $target)' "$TMP/body" >/dev/null; then
  echo "error: a probe type already exists; refusing to overwrite it" >&2
  exit 2
fi
cp "$TMP/body" "$TMP/managed-before.json"

HOOK_SOURCE='function hasValue(value) {
  return value !== null && typeof value !== "undefined";
}
var explicit = openidm.read(String(resourceName), null, ["relDefaultFalse/label", "relDefaultTrue/label"]);
object.hookObjectFalse = hasValue(object.relDefaultFalse);
object.hookObjectTrue = hasValue(object.relDefaultTrue);
object.hookOldFalse = hasValue(oldObject.relDefaultFalse);
object.hookOldTrue = hasValue(oldObject.relDefaultTrue);
object.hookReadFalse = hasValue(explicit.relDefaultFalse);
object.hookReadTrue = hasValue(explicit.relDefaultTrue);
object.hookObjectTrueLabel = hasValue(object.relDefaultTrue) && object.relDefaultTrue.label === "Target one";
object.hookOldTrueLabel = hasValue(oldObject.relDefaultTrue) && oldObject.relDefaultTrue.label === "Target one";
object.hookReadFalseLabel = hasValue(explicit.relDefaultFalse) && explicit.relDefaultFalse.label === "Target one";
object.hookReadTrueLabel = hasValue(explicit.relDefaultTrue) && explicit.relDefaultTrue.label === "Target one";'

jq \
  --arg source "$SOURCE_OBJ" \
  --arg target "$TARGET_OBJ" \
  --arg hook "$HOOK_SOURCE" \
  '.objects += [
    {
      name: $target,
      schema: {
        type: "object",
        title: "AIC hook relationship probe target",
        properties: {label: {type: "string", searchable: true}},
        required: ["label"],
        order: ["label"]
      }
    },
    {
      name: $source,
      schema: {
        type: "object",
        title: "AIC hook relationship probe source",
        properties: {
          note: {type: "string"},
          relDefaultFalse: {
            type: "relationship",
            title: "Not returned by default",
            returnByDefault: false,
            reverseRelationship: false,
            validate: true,
            resourceCollection: [{
              path: ("managed/" + $target),
              query: {fields: [], queryFilter: "true", sortKeys: []}
            }],
            properties: {
              _ref: {type: "string"},
              _refProperties: {type: "object", properties: {_id: {type: "string"}}}
            }
          },
          relDefaultTrue: {
            type: "relationship",
            title: "Returned by default",
            returnByDefault: true,
            reverseRelationship: false,
            validate: true,
            resourceCollection: [{
              path: ("managed/" + $target),
              query: {fields: [], queryFilter: "true", sortKeys: []}
            }],
            properties: {
              _ref: {type: "string"},
              _refProperties: {type: "object", properties: {_id: {type: "string"}}}
            }
          },
          hookObjectFalse: {type: "boolean"},
          hookObjectTrue: {type: "boolean"},
          hookOldFalse: {type: "boolean"},
          hookOldTrue: {type: "boolean"},
          hookReadFalse: {type: "boolean"},
          hookReadTrue: {type: "boolean"},
          hookObjectTrueLabel: {type: "boolean"},
          hookOldTrueLabel: {type: "boolean"},
          hookReadFalseLabel: {type: "boolean"},
          hookReadTrueLabel: {type: "boolean"}
        },
        required: [],
        order: [
          "note", "relDefaultFalse", "relDefaultTrue",
          "hookObjectFalse", "hookObjectTrue", "hookOldFalse", "hookOldTrue",
          "hookReadFalse", "hookReadTrue", "hookObjectTrueLabel", "hookOldTrueLabel",
          "hookReadFalseLabel", "hookReadTrueLabel"
        ]
      },
      onUpdate: {type: "text/javascript", source: $hook}
    }
  ]' "$TMP/managed-before.json" > "$TMP/managed-with-probe.json"

req_file PUT /openidm/config/managed "$TMP/managed-with-probe.json"
check "config write" "200" "$CODE"
[ "$CODE" = 200 ] || exit 2
INSTALLED=1

printf '  waiting for both types and the hook to become active'
ready=""
for i in $(seq 1 40); do
  req GET "/openidm/managed/$SOURCE_OBJ?_queryFilter=true&_pageSize=1"
  source_code="$CODE"
  req GET "/openidm/managed/$TARGET_OBJ?_queryFilter=true&_pageSize=1"
  if [ "$source_code" = 200 ] && [ "$CODE" = 200 ]; then
    ready=yes
    break
  fi
  printf '.'
  sleep 3
done
printf ' %s\n' "${ready:-gave up}"
[ -n "$ready" ] || exit 2

say "create target and source records"
req PUT "/openidm/managed/$TARGET_OBJ/target1" '{"label":"Target one"}'
check "target create" "201" "$CODE"

source_one="$(jq -cn --arg target "$TARGET_OBJ" '{
  note: "before update",
  relDefaultFalse: {_ref:("managed/" + $target + "/target1")},
  relDefaultTrue: {_ref:("managed/" + $target + "/target1")}
}')"
req PUT "/openidm/managed/$SOURCE_OBJ/person1" "$source_one"
check "person1 create" "201" "$CODE"

say "PATCH an unrelated scalar and inspect what onUpdate saw"
# A fresh type can serve records before its latest hook registry is active.
# Retry with a changing note until the observation fields appear.
hook_ready=""
for i in $(seq 1 20); do
  req PATCH "/openidm/managed/$SOURCE_OBJ/person1" \
    "[{\"operation\":\"replace\",\"field\":\"/note\",\"value\":\"update-$i\"}]"
  if [ "$CODE" = 200 ] && jq -e 'has("hookReadTrue")' "$TMP/body" >/dev/null; then
    hook_ready=yes
    break
  fi
  sleep 3
done
check "onUpdate hook became active" "yes" "$hook_ready"
jq '{
  hookObjectFalse, hookObjectTrue, hookOldFalse, hookOldTrue,
  hookReadFalse, hookReadTrue, hookObjectTrueLabel, hookOldTrueLabel,
  hookReadFalseLabel, hookReadTrueLabel
}' "$TMP/body"
check "object omits returnByDefault:false relationship" "false" \
  "$(jq -r '.hookObjectFalse' "$TMP/body")"
check "object includes returnByDefault:true relationship" "true" \
  "$(jq -r '.hookObjectTrue' "$TMP/body")"
check "oldObject omits returnByDefault:false relationship" "false" \
  "$(jq -r '.hookOldFalse' "$TMP/body")"
check "oldObject includes returnByDefault:true relationship" "true" \
  "$(jq -r '.hookOldTrue' "$TMP/body")"
check "explicit read obtains returnByDefault:false relationship" "true" \
  "$(jq -r '.hookReadFalse' "$TMP/body")"
check "explicit read obtains returnByDefault:true relationship" "true" \
  "$(jq -r '.hookReadTrue' "$TMP/body")"
check "default-returned relationship omits target field on object" "false" \
  "$(jq -r '.hookObjectTrueLabel' "$TMP/body")"
check "default-returned relationship omits target field on oldObject" "false" \
  "$(jq -r '.hookOldTrueLabel' "$TMP/body")"
check "projected read expands target field for false relationship" "true" \
  "$(jq -r '.hookReadFalseLabel' "$TMP/body")"
check "projected read expands target field for true relationship" "true" \
  "$(jq -r '.hookReadTrueLabel' "$TMP/body")"

echo
if [ "$FAILED" -eq 0 ]; then
  echo "all assertions passed"
else
  echo "$FAILED assertion(s) FAILED"
fi
exit "$FAILED"
