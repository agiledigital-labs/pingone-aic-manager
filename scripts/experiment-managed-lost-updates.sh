#!/usr/bin/env bash
# experiment-managed-lost-updates.sh
#
# Question this answers:
#   `PUT /openidm/config/managed` is a whole-document replace, so every schema
#   change is a read-modify-write. Is a `GET` immediately after a successful
#   `PUT` guaranteed to reflect that `PUT`? If not, back-to-back schema writes
#   silently lose each other, because write N reads a document that does not yet
#   contain write N-1 and then persists that stale document.
#
# Answer (sandbox, 2026-08-05): NO. `config/managed` is not read-your-writes
#   consistent, and two distinct failures are observable:
#
#     after add f1   ["f1"]
#     after add f2   ["f2"]           <- f1 silently lost
#     after add f3   ["f2","f3"]
#     ...
#     after add f8   ["f2".."f8"]
#     settled (+10s) ["f2".."f7"]     <- f8 vanished with no write at all
#
#   1. Lost updates: the read backing write N returned the pre-write-(N-1) state.
#      Every call returned 2xx. Nothing surfaces the loss.
#   2. Reads going backwards: a property confirmed present immediately after its
#      write is absent from a later read, with no intervening write.
#
#   The reads here go straight to the tenant with curl, bypassing the local `aic
#   agent` that proxies the CLI's own calls — so this is the tenant's config
#   store, not the daemon.
#
#   This contradicts the "config read-back is effectively immediate ... strong
#   consistency for the stored config" note in docs/api/10-managed-objects.md
#   (verified 2026-06-14 off a single ~164ms observation). See Q14 in
#   99-quirks-and-open-questions.md.
#
#   The loss is not confined to one window. Two further runs:
#
#     * Without waiting after `object create`, the very first `field add` is the
#       one lost, every time — the new object type is instantiated
#       asynchronously, and a write landing during that window does not survive.
#     * Waiting for the type to answer queries first (9s) saved that first field,
#       but a later add in the same run (f7 of 8) was still lost. So the
#       instantiation window is one common case, not the whole story.
#
# Consequence: **a 200 on `PUT /openidm/config/managed` does not mean the change
#   is durable.** Any tool doing read-modify-write against it — this one, and the
#   admin console — can silently discard a change it just made. A write path that
#   cares must re-read and confirm its own change landed, with a bounded retry,
#   rather than trusting the status code. Waiting for a newly created object type
#   to instantiate before writing its fields is worth doing too, but it is a
#   mitigation, not a fix.
#
# Usage:
#   scripts/experiment-managed-lost-updates.sh
#   FIELDS=20 scripts/experiment-managed-lost-updates.sh
#   SETTLE=30 scripts/experiment-managed-lost-updates.sh
#
# Env:
#   AIC     aic binary            (default ./target/debug/aic)
#   OBJ     throwaway object      (default test_lostupdate)
#   FIELDS  sequential adds       (default 8)
#   SETTLE  seconds to wait before the final read (default 10)
#   KEEP    skip cleanup
#
# Exits 0 if every add survived and the settled read matches, 1 otherwise. A
# clean run is evidence the tenant behaved this time, not proof it always will.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
AIC="${AIC:-$ROOT/target/debug/aic}"
OBJ="${OBJ:-test_lostupdate}"
FIELDS="${FIELDS:-8}"
SETTLE="${SETTLE:-10}"

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

# Read straight from the tenant: the point is to observe the config store, not
# whatever the CLI's proxy would report.
props() {
  curl -sS -H "Authorization: Bearer $TOKEN" "$BASE/openidm/config/managed" \
    | jq -c --arg n "$OBJ" '[.objects[] | select(.name==$n) | .schema.properties | keys[]]'
}

echo "tenant: $BASE"
echo "object: $OBJ   fields: $FIELDS   settle: ${SETTLE}s"
echo

"$AIC" managed object delete "$OBJ" --yes >/dev/null 2>&1
"$AIC" managed object create "$OBJ" --title "Lost update probe" --yes >/dev/null \
  || { echo "error: could not create $OBJ" >&2; exit 2; }

expected=""
refused=0
ever_seen=""
for i in $(seq 1 "$FIELDS"); do
  # Do NOT swallow stderr. Since `replace_managed_confirmed` landed, a write the
  # tenant fails to persist is a hard error rather than a silent loss, and that
  # error is the single most interesting line this script can print.
  if ! "$AIC" managed field add "$OBJ.f$i" --type string --yes >/dev/null; then
    refused=$((refused + 1))
  fi
  expected="$expected f$i"
  seen="$(props)"
  printf 'after add %-4s %s\n' "f$i" "$seen"
  # Track every field name ever observed stored. This is what separates the two
  # failure modes at the end: a field that was never once visible means our
  # confirmation missed a write path; a field that was visible and is gone means
  # the tenant rolled the document back, which no client-side retry can prevent.
  ever_seen="$ever_seen $(printf '%s' "$seen" | jq -r '.[]' | tr '\n' ' ')"
done

printf '\nsettling %ss...\n' "$SETTLE"
sleep "$SETTLE"
settled="$(props)"
printf 'settled       %s\n' "$settled"

# Compare as sorted sets. `jq keys` is lexicographic, so with more than nine
# fields "f10" sorts before "f2" and a naive string compare reports loss that
# isn't there.
# shellcheck disable=SC2086
want="$(printf '%s\n' $expected | jq -R . | jq -sc 'sort')"
settled="$(printf '%s' "$settled" | jq -c 'sort')"
printf '\nexpected      %s\n' "$want"

status=0
echo
if [ "$settled" = "$want" ]; then
  echo "every add survived this run"
else
  # Classify what went missing, because the two causes have different owners.
  reverted="" never=""
  for f in $expected; do
    printf '%s' "$settled" | jq -e --arg f "$f" 'index($f) != null' >/dev/null && continue
    case " $ever_seen " in
      *" $f "*) reverted="$reverted $f" ;;
      *) never="$never $f" ;;
    esac
  done

  [ "$refused" -gt 0 ] && {
    echo "$refused write(s) were REFUSED by aic after the tenant failed to persist"
    echo "them. That is confirm-after-write doing its job: the loss is loud, not"
    echo "silent, and re-running the add is safe because the writes are idempotent."
    echo
  }
  [ -n "$reverted" ] && {
    echo "ROLLED BACK by the tenant:$reverted"
    echo "  These were observed stored and later vanished. Confirm-after-write"
    echo "  cannot prevent this — the write landed, was verified, and the config"
    echo "  store subsequently reverted to an older snapshot. Objects deleted"
    echo "  hours earlier have also been seen to reappear. This is a platform"
    echo "  fault to raise with Ping, not something a client can retry around."
    status=1
    echo
  }
  [ -n "$never" ] && {
    echo "MISSING, yet aic reported success:$never"
    echo "  aic only returns success after reading the change back, and a stale"
    echo "  read can return old data but cannot invent a field that was never"
    echo "  written — so each of these WAS stored when aic looked. It then either"
    echo "  got rolled back, or the confirming read and this script's read hit"
    echo "  replicas that disagree. This script cannot separate those two, and"
    echo "  both are platform-side: no client-side retry closes them. Intermittent"
    echo "  — short runs often pass cleanly, longer ones lose one or two."
    status=1
  }
fi

if [ -z "${KEEP:-}" ]; then
  "$AIC" managed object delete "$OBJ" --yes >/dev/null 2>&1 \
    && echo "removed $OBJ" || echo "WARNING: could not remove $OBJ"
fi
exit "$status"
