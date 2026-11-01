#!/usr/bin/env bash
# experiment-managed-lifecycle-trace.sh
#
# Question this answers:
#   In what order does IDM run every managed-object hook, object-level and
#   property-level, for each operation, and what does each hook see?
#
# Method:
#   Install a throwaway subject type with every object hook (onCreate, onRead,
#   onRetrieve, onUpdate, onDelete, onValidate, onStore, onSync, postCreate,
#   postUpdate, postDelete), property hooks (onValidate, onStore, onRetrieve) and
#   a custom action, plus an event type. Every hook writes one event recording
#   its phase, a nanoTime sequence number, and the typeof and shape of every
#   candidate binding. The script then drives a fixed sequence of requests and
#   prints, for each one, the HTTP outcome and the events it produced in order.
#
#   This is a recorder, not an assertion suite: it is how the emulator's hook
#   order and bindings were measured. Its output is the evidence.
#
# Everything is deleted and both type definitions are removed unless KEEP=1.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
AIC="${AIC:-aic}"
SUBJECT_OBJ="${SUBJECT_OBJ:-zzemu_subject}"
EVENT_OBJ="${EVENT_OBJ:-zzemu_event}"

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
INSTALLED=0
S="/openidm/managed/$SUBJECT_OBJ"

say() { printf '\n== %s\n' "$1"; }

# req METHOD PATH [BODY] [EXTRA-HEADER] -> $CODE and $TMP/body
req() {
  local method="$1" path="$2" body="${3:-}" extra="${4:-X-Probe: 1}"
  if [ -n "$body" ]; then
    CODE="$(curl -sS -o "$TMP/body" -w '%{http_code}' -X "$method" \
      -H "Authorization: Bearer $TOKEN" -H 'Accept-API-Version: resource=1.0' \
      -H 'Content-Type: application/json' -H "$extra" -d "$body" "$BASE$path")"
  else
    CODE="$(curl -sS -o "$TMP/body" -w '%{http_code}' -X "$method" \
      -H "Authorization: Bearer $TOKEN" -H 'Accept-API-Version: resource=1.0' \
      -H "$extra" "$BASE$path")"
  fi
}

req_file() {
  CODE="$(curl -sS -o "$TMP/body" -w '%{http_code}' -X "$1" \
    -H "Authorization: Bearer $TOKEN" -H 'Accept-API-Version: resource=1.0' \
    -H 'Content-Type: application/json' --data-binary @"$3" "$BASE$2")"
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

# Print the request outcome, then the events it produced in sequence order,
# then clear the events so the next step starts empty.
show() {
  local label="$1"
  say "$label"
  printf '  HTTP %s  %s\n' "$CODE" "$(jq -c 'del(._rev)' "$TMP/body" 2>/dev/null | cut -c1-600)"
  cp "$TMP/body" "$TMP/last-response"
  curl -sS -H "Authorization: Bearer $TOKEN" -H 'Accept-API-Version: resource=1.0' \
    "$BASE/openidm/managed/$EVENT_OBJ?_queryFilter=true&_fields=phase,seq,info" > "$TMP/events"
  jq -r '.result | sort_by(.seq | tonumber) | .[] | "  - \(.phase)  \(.info)"' "$TMP/events" | cut -c1-900
  delete_records "$EVENT_OBJ"
  cp "$TMP/last-response" "$TMP/body"
}

# shellcheck disable=SC2329
remove_types() {
  req GET /openidm/config/managed
  [ "$CODE" = 200 ] || return 1
  jq --arg subject "$SUBJECT_OBJ" --arg event "$EVENT_OBJ" \
    '.objects |= map(select(.name != $subject and .name != $event))' \
    "$TMP/body" > "$TMP/cleanup.json"
  req_file PUT /openidm/config/managed "$TMP/cleanup.json"
  [ "$CODE" = 200 ]
}

# shellcheck disable=SC2329
cleanup() {
  local exit_code=$?
  trap - EXIT
  if [ -n "${KEEP:-}" ]; then
    echo "KEEP set — leaving $SUBJECT_OBJ and $EVENT_OBJ behind"
    rm -rf "$TMP"; exit "$exit_code"
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

# The recorder every hook runs. PHASE is substituted per hook. It never
# serialises `context` or `request` wholesale: the context chain carries the
# caller's bearer token.
cat > "$TMP/recorder.js" <<'EOF'
(function (phase) {
  var info = {};
  var names = ["object", "oldObject", "newObject", "request", "context",
    "resourceName", "value", "property", "propertyName", "revision", "input"];
  for (var i = 0; i < names.length; i++) {
    try { info[names[i]] = eval("typeof " + names[i]); } catch (e) { info[names[i]] = "ERR"; }
  }
  function shape(v) {
    if (v === null) return "null";
    if (typeof v !== "object") return typeof v + ":" + String(v);
    return Object.prototype.toString.call(v) + "{" + Object.keys(v).join(",") + "}";
  }
  try { if (typeof object !== "undefined") info.objectShape = shape(object); } catch (e) { info.objectShape = "ERR " + e; }
  try { if (typeof oldObject !== "undefined") info.oldShape = shape(oldObject); } catch (e) { info.oldShape = "ERR " + e; }
  try { if (typeof newObject !== "undefined") info.newShape = shape(newObject); } catch (e) { info.newShape = "ERR " + e; }
  try { if (typeof value !== "undefined") info.valueShape = shape(value); } catch (e) { info.valueShape = "ERR " + e; }
  try { if (typeof property !== "undefined") info.propertyShape = shape(property); } catch (e) { info.propertyShape = "ERR " + e; }
  try { if (typeof propertyName !== "undefined") info.propertyName = String(propertyName); } catch (e) {}
  try { if (typeof resourceName !== "undefined") info.resourceName = String(resourceName) + " (" + Object.prototype.toString.call(resourceName) + ")"; } catch (e) {}
  try { if (typeof request !== "undefined" && request !== null) info.requestMethod = String(request.method) + " " + Object.prototype.toString.call(request); } catch (e) {}
  try { if (typeof object !== "undefined" && object !== null) { info.objV = object.v; info.objStore = object.pstore; info.objRev = typeof object._rev; } } catch (e) {}
  openidm.create("managed/__EVENT__", null, {
    phase: phase, seq: String(java.lang.System.nanoTime()), info: JSON.stringify(info)
  });
})("__PHASE__");
EOF
sed -i "s/__EVENT__/$EVENT_OBJ/" "$TMP/recorder.js"

# hook PHASE [EXTRA-JS] -> a script object whose source records, then runs EXTRA.
hook() {
  local src
  src="$(sed "s/__PHASE__/$1/" "$TMP/recorder.js")"
  jq -n --arg s "$src
${2:-}" '{type: "text/javascript", source: $s}'
}

say "install subject and event types"
req GET /openidm/config/managed
[ "$CODE" = 200 ] || { echo "error: config/managed GET returned $CODE" >&2; exit 2; }
if jq -e --arg subject "$SUBJECT_OBJ" --arg event "$EVENT_OBJ" \
  '.objects[] | select(.name == $subject or .name == $event)' "$TMP/body" >/dev/null; then
  echo "error: a probe type already exists; refusing to overwrite it" >&2
  exit 2
fi
cp "$TMP/body" "$TMP/managed-before.json"

jq -n \
  --argjson onCreate "$(hook onCreate 'object.setByOnCreate = "yes"; if (object.mode === "throw418") { throw { code: 418, message: "tea", detail: { a: 1 } }; } if (object.mode === "throwError") { throw new Error("plain error"); }')" \
  --argjson onRead "$(hook onRead 'object.setByOnRead = "yes";')" \
  --argjson onRetrieve "$(hook onRetrieve 'object.setByOnRetrieve = "yes";')" \
  --argjson onUpdate "$(hook onUpdate 'object.setByOnUpdate = "yes";')" \
  --argjson onDelete "$(hook onDelete)" \
  --argjson onValidate "$(hook onValidate)" \
  --argjson onStore "$(hook onStore 'object.setByOnStore = "yes";')" \
  --argjson onSync "$(hook onSync)" \
  --argjson postCreate "$(hook postCreate)" \
  --argjson postUpdate "$(hook postUpdate)" \
  --argjson postDelete "$(hook postDelete)" \
  --argjson pValidate "$(hook prop.pval.onValidate 'if (property === "bad") { throw "bad pval"; }')" \
  --argjson pStore "$(hook prop.pstore.onStore 'property + "-stored";')" \
  --argjson pNone "$(hook prop.pnone.onStore 'var ignored = 1;')" \
  --argjson pRetrieve "$(hook prop.pret.onRetrieve 'property + "-retrieved";')" \
  --arg ping "$(sed 's/__PHASE__/action.ping/' "$TMP/recorder.js")
({ hello: 1 });" \
  --arg subject "$SUBJECT_OBJ" --arg event "$EVENT_OBJ" '
  [
    { name: $event,
      schema: { type: "object", title: "zzemu lifecycle events",
        properties: { phase: {type: "string"}, seq: {type: "string"}, info: {type: "string"} },
        order: ["phase", "seq", "info"] } },
    { name: $subject,
      schema: { type: "object", title: "zzemu lifecycle subject",
        properties: {
          v: {type: "string"}, mode: {type: "string"},
          pval: {type: "string", onValidate: $pValidate},
          pstore: {type: "string", onStore: $pStore},
          pnone: {type: "string", nullable: true, onStore: $pNone},
          pret: {type: "string", onRetrieve: $pRetrieve},
          setByOnCreate: {type: "string"}, setByOnUpdate: {type: "string"},
          setByOnStore: {type: "string"}, setByOnRead: {type: "string"},
          setByOnRetrieve: {type: "string"}
        },
        order: ["v", "mode", "pval", "pstore", "pnone", "pret", "setByOnCreate",
          "setByOnUpdate", "setByOnStore", "setByOnRead", "setByOnRetrieve"] },
      onCreate: $onCreate, onRead: $onRead, onRetrieve: $onRetrieve,
      onUpdate: $onUpdate, onDelete: $onDelete, onValidate: $onValidate,
      onStore: $onStore, onSync: $onSync, postCreate: $postCreate,
      postUpdate: $postUpdate, postDelete: $postDelete,
      actions: { ping: { type: "text/javascript", source: $ping } } }
  ]' > "$TMP/types.json"

jq --slurpfile t "$TMP/types.json" '.objects += $t[0]' "$TMP/managed-before.json" > "$TMP/managed-with-probe.json"
req_file PUT /openidm/config/managed "$TMP/managed-with-probe.json"
printf '  config write: HTTP %s\n' "$CODE"
[ "$CODE" = 200 ] || exit 2
INSTALLED=1

printf '  waiting for both types and hooks to become active'
ready=""
for _attempt in $(seq 1 40); do
  req PUT "$S/warmup" '{"v":"warm","pstore":"w","pnone":"w","pval":"ok","pret":"w"}'
  warm="HTTP $CODE $(head -c 400 "$TMP/body")"
  if [ "$CODE" = 201 ]; then
    req GET "/openidm/managed/$EVENT_OBJ?_queryFilter=phase+eq+\"postCreate\"&_fields=_id"
    if [ "$CODE" = 200 ] && [ "$(jq '.resultCount' "$TMP/body")" -ge 1 ]; then ready=yes; fi
  fi
  delete_records "$SUBJECT_OBJ"
  delete_records "$EVENT_OBJ"
  [ -n "$ready" ] && break
  printf '.'; sleep 3
done
printf ' %s\n' "${ready:-gave up}"
[ -n "$ready" ] || { echo "  last warm-up create: $warm"; exit 2; }

BODY='{"v":"a","pval":"ok","pstore":"a","pnone":"a","pret":"a"}'

req PUT "$S/s1" "$BODY" "If-None-Match: *"
show "1. create s1 (PUT, If-None-Match *)"

req GET "$S/s1"
show "2. read s1"

req GET "$S/s1?_fields=v"
show "3. read s1 with _fields=v"

req GET "$S?_queryFilter=true"
show "4. query"

req GET "$S?_queryFilter=true&executeOnRetrieve=true"
show "5. query with executeOnRetrieve=true"

req GET "$S/s1"; cp "$TMP/body" "$TMP/s1.json"
req_file PUT "$S/s1" "$TMP/s1.json"
show "6. PUT the exact body just read (incl. _rev)"

jq 'del(._rev)' "$TMP/s1.json" > "$TMP/s1-norev.json"
req_file PUT "$S/s1" "$TMP/s1-norev.json"
show "7. PUT the body just read, without _rev"

req GET "$S/s1"; REV="$(jq -r '._rev' "$TMP/body")"
req PUT "$S/s1" '{"v":"a2","pval":"ok","pstore":"a","pnone":"a","pret":"a"}' "If-Match: $REV"
show "7b. PUT v=a2 with If-Match"

req GET "$S/s1"; jq '.v = "a3"' "$TMP/body" > "$TMP/s1-a3.json"
req_file PUT "$S/s1" "$TMP/s1-a3.json"
show "7c. PUT v=a3 with _rev in the body, no If-Match"

req PATCH "$S/s1" '[{"operation":"replace","field":"/v","value":"b"}]'
show "8. PATCH v=b"

req PATCH "$S/s1" '[{"operation":"replace","field":"/v","value":"b"}]'
show "9. PATCH v=b again (no change)"

req PUT "$S/s1" '{"v":"c","pval":"ok","pstore":"c","pnone":"c","pret":"c"}'
show "10. PUT v=c (full replace)"

req POST "$S/s1?_action=ping" '{}'
show "11. custom action ping on s1"

req POST "$S?_action=ping" '{}'
show "12. custom action ping on the collection"

req PUT "$S/s2" '{"v":"x","mode":"throw418","pval":"ok"}'
show "13. create with onCreate throwing {code:418}"

req PUT "$S/s3" '{"v":"x","mode":"throwError","pval":"ok"}'
show "14. create with onCreate throwing new Error"

req PUT "$S/s4" '{"v":"x","pval":"bad"}'
show "15. create with property onValidate throwing a string"

req PUT "$S/s1" "$BODY" "If-None-Match: *"
show "16. create s1 again with If-None-Match *"

req POST "$S?_action=create" '{"v":"server-id","pval":"ok"}'
show "17. create with a server-assigned id"

req DELETE "$S/s1"
show "18. delete s1"

req GET "/openidm/managed/$SUBJECT_OBJ?_queryFilter=true&_fields=_id,v"
show "19. remaining subjects"
