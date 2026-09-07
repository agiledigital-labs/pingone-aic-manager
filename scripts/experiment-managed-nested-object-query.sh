#!/usr/bin/env bash
# experiment-managed-nested-object-query.sh
#
# Question this answers:
#   Can a custom managed object be filtered and sorted by a child of an
#   object-valued property (for example `name/last`)?
#
# Method:
#   Install one throwaway custom type with `name: {first, middle, last}`, create
#   three records, filter by `/name/last`, and sort by `name/last`. The records
#   and type definition are removed unless KEEP=1.
#
# This proves query behaviour, not physical index use. Indexing is a repository
# characteristic; docs/api/10-managed-objects.md records Ping's AIC generic-
# object storage guarantee and the contrasting unindexed user-custom case.
#
# Usage:
#   scripts/experiment-managed-nested-object-query.sh
#   AIC=/path/to/aic scripts/experiment-managed-nested-object-query.sh
#   KEEP=1 scripts/experiment-managed-nested-object-query.sh

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
AIC="${AIC:-aic}"
OBJ="${OBJ:-test_nested_query_probe}"

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

# query FILTER [SORT] -> $CODE and $TMP/body
query() {
  local filter="$1" sort="${2:-}"
  local args=(
    -sS -G -o "$TMP/body" -w '%{http_code}'
    -H "Authorization: Bearer $TOKEN"
    -H 'Accept-API-Version: resource=1.0'
    --data-urlencode "_queryFilter=$filter"
    --data-urlencode '_fields=_id,name'
    --data-urlencode '_pageSize=20'
  )
  [ -n "$sort" ] && args+=(--data-urlencode "_sortKeys=$sort")
  CODE="$(curl "${args[@]}" "$BASE/openidm/managed/$OBJ")"
}

remove_type() {
  req GET /openidm/config/managed
  [ "$CODE" = 200 ] || return 1
  cp "$TMP/body" "$TMP/cleanup-before.json"
  jq --arg object "$OBJ" '.objects |= map(select(.name != $object))' \
    "$TMP/cleanup-before.json" > "$TMP/cleanup.json"
  req_file PUT /openidm/config/managed "$TMP/cleanup.json"
  [ "$CODE" = 200 ]
}

cleanup() {
  local exit_code=$?
  trap - EXIT
  if [ -n "${KEEP:-}" ]; then
    echo
    echo "KEEP set — leaving $OBJ behind"
    rm -rf "$TMP"
    exit "$exit_code"
  fi

  say "cleanup"
  if [ "$INSTALLED" -eq 1 ]; then
    req GET "/openidm/managed/$OBJ?_queryFilter=true&_fields=_id"
    if [ "$CODE" = 200 ]; then
      jq -r '.result[]._id' "$TMP/body" > "$TMP/ids"
      while IFS= read -r id; do
        [ -n "$id" ] && req DELETE "/openidm/managed/$OBJ/$id"
      done < "$TMP/ids"
    fi
    if remove_type; then
      echo "  removed records and throwaway type"
    else
      echo "  WARNING: could not remove the throwaway type" >&2
      exit_code=1
    fi
  fi
  rm -rf "$TMP"
  exit "$exit_code"
}
trap cleanup EXIT

echo "context: $($AIC --no-prompt ctx current 2>/dev/null || echo unknown)"
echo "object: $OBJ"

say "install the throwaway custom managed-object type"
req GET /openidm/config/managed
if [ "$CODE" != 200 ]; then
  echo "error: config/managed GET returned $CODE" >&2
  exit 2
fi
if jq -e --arg object "$OBJ" '.objects[] | select(.name == $object)' \
  "$TMP/body" >/dev/null; then
  echo "error: the probe type already exists; refusing to overwrite it" >&2
  exit 2
fi
cp "$TMP/body" "$TMP/managed-before.json"

jq --arg object "$OBJ" '.objects += [{
  name: $object,
  schema: {
    type: "object",
    title: "AIC nested object query probe",
    properties: {
      name: {
        type: "object",
        title: "Name",
        searchable: true,
        properties: {
          first: {type: "string", searchable: true},
          middle: {type: "string", searchable: true},
          last: {type: "string", searchable: true}
        },
        required: ["first", "last"],
        order: ["first", "middle", "last"]
      }
    },
    required: ["name"],
    order: ["name"]
  }
}]' "$TMP/managed-before.json" > "$TMP/managed-with-probe.json"

req_file PUT /openidm/config/managed "$TMP/managed-with-probe.json"
check "config write" "200" "$CODE"
[ "$CODE" = 200 ] || exit 2
INSTALLED=1

printf '  waiting for the type to become active'
ready=""
for _ in $(seq 1 40); do
  req GET "/openidm/managed/$OBJ?_queryFilter=true&_pageSize=1"
  if [ "$CODE" = 200 ]; then
    ready=yes
    break
  fi
  printf '.'
  sleep 3
done
printf ' %s\n' "${ready:-gave up}"
[ -n "$ready" ] || exit 2

say "create three nested-name records"
req PUT "/openidm/managed/$OBJ/person1" \
  '{"name":{"first":"Alice","middle":"Q","last":"Smith"}}'
check "person1 create" "201" "$CODE"
req PUT "/openidm/managed/$OBJ/person2" \
  '{"name":{"first":"Bob","middle":"R","last":"Jones"}}'
check "person2 create" "201" "$CODE"
req PUT "/openidm/managed/$OBJ/person3" \
  '{"name":{"first":"Carol","middle":"S","last":"Smith"}}'
check "person3 create" "201" "$CODE"

say "filter and sort by the child of an object-valued field"
query '/name/last eq "Smith"' 'name/last,_id'
check "nested equality query status" "200" "$CODE"
check "nested equality query returns person1 and person3 only" \
  '["person1","person3"]' \
  "$(jq -c '[.result[]._id] | sort' "$TMP/body")"
jq -c '[.result[] | {id: ._id, name}]' "$TMP/body"

query 'true' 'name/last,_id'
check "nested sort query status" "200" "$CODE"
check "nested sort orders Jones before Smith" \
  '["person2","person1","person3"]' \
  "$(jq -c '[.result[]._id]' "$TMP/body")"

echo
if [ "$FAILED" -eq 0 ]; then
  echo "all assertions passed"
else
  echo "$FAILED assertion(s) FAILED"
fi
exit "$FAILED"
