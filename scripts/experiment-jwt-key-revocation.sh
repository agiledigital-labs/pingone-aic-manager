#!/usr/bin/env bash
# experiment-jwt-key-revocation.sh
#
# Question this answers:
#   When `aic jwt-bearer key remove` takes a key out of the realm's Trusted JWT
#   Issuer `jwkSet`, how long does AM keep minting tokens signed with it?
#   Removal is a security action; if it takes an hour to bite, that has to be
#   written down rather than assumed.
#
# Why it isn't already answered:
#   Probing on 2026-08-07 confirmed the WRITE lands — AM stores `{"keys":[]}`
#   and `key list` shows nothing — but a token still minted immediately after.
#   Three confounders were in play and none was isolated:
#     1. `jwksCacheTimeout` on the issuer (3600000ms = 1h positive cache).
#     2. `jwkStoreCacheMissCacheTime` (60000ms = 1m negative cache).
#     3. A propagation delay of ~20s on freshly created OAuth2 clients, during
#        which the token endpoint answers `invalid_client` regardless of key
#        state — which looks exactly like a revoked key and inverted two
#        earlier runs.
#   The earlier probe also varied two things at once (it relabelled the `kid` on
#   the SAME key material, which AM accepts because it falls back to trying
#   every key in the set). This script holds one variable at a time and settles
#   the client-propagation confounder up front.
#
# Method:
#   1. Create a throwaway client WITH the jwt-bearer grant, then wait out the
#      propagation delay and prove it mints. This is the positive control; the
#      run aborts if it never goes green, because every later reading depends
#      on the client being live.
#   2. Snapshot the local private key so the tenant can be restored exactly.
#   3. Remove the published key, and confirm via a direct read that the jwkSet
#      really no longer contains that kid. (Distinguishes "write didn't land"
#      from "write landed, AM still accepts".)
#   4. Poll `aic auth` until it is refused, recording elapsed seconds. Re-assert
#      the positive control periodically against a SECOND, still-published key
#      so a refusal caused by something unrelated (tenant hiccup, idle lock)
#      cannot be misread as revocation.
#   5. Restore: republish the original key, delete the throwaway client, shred
#      the exported key file.
#
# Exit status is the number of failed assertions, so this works as a check and
# not just a report. A run that reaches TIMEOUT without a refusal is a RESULT,
# not a failure: it means revocation is slower than the window observed, which
# is itself the thing worth knowing.
#
# Token/auth: uses the running `aic agent`. Unlocks from $AGENT_PASSWORD when
#   set; otherwise run `aic login` first.
#
# Cost: up to TIMEOUT seconds of mostly sleeping (default 3900 = 65min, chosen
#   to exceed the 1h positive cache). Set TIMEOUT=300 for a smoke run that only
#   proves the harness works.
#
# !! THIS SCRIPT REMOVES A LIVE SIGNING KEY FROM A SHARED ISSUER !!
#   It restores the key on exit, but a removed key is not revoked promptly (that
#   is the thing being measured), and there is no evidence either way on how
#   quickly a restored one becomes usable again. Do not run it while anyone
#   needs `aic auth` on the target realm.
#
#   A "quick smoke run" costs the same key removal as the real one. There is no
#   cheap mode.
#
#   NOTE (2026-08-07): an earlier version of this header claimed the cache
#   "cuts both ways", citing an episode where auth stayed broken after a
#   restore. That was retracted — the clients in that episode were
#   `client_secret_basic`, which `aic auth` cannot use at all. Keep that
#   confound out of any run: create probe clients with
#   `tokenEndpointAuthMethod: client_secret_post` (see the positive control
#   below), or you will measure the wrong thing.
#
# Usage:
#   scripts/experiment-jwt-key-revocation.sh
#   TIMEOUT=300 scripts/experiment-jwt-key-revocation.sh
#   REALM=bravo scripts/experiment-jwt-key-revocation.sh
#
# Env:
#   REALM     realm to probe                     (default alpha)
#   TENANT    tenant name                        (default: current context)
#   SUBJECT   username to mint as                (default testuser)
#   TIMEOUT   seconds to poll before giving up   (default 3900)
#   INTERVAL  seconds between polls              (default 30)
#   KEEP=1    skip cleanup (leave the client behind for inspection)

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
AIC="$ROOT/target/debug/aic"

REALM="${REALM:-alpha}"
SUBJECT="${SUBJECT:-testuser}"
TIMEOUT="${TIMEOUT:-3900}"
INTERVAL="${INTERVAL:-30}"
CLIENT="test_revocation_probe"
# Long enough to clear the ~20s client-propagation delay measured 2026-08-07.
SETTLE=40

FAILURES=0
WORK="$(mktemp -d)"
RESTORE_NEEDED=0

note()  { printf '%s\n' "$*"; }
check() { if [ "$1" = "ok" ]; then printf '  PASS  %s\n' "$2"; else printf '  FAIL  %s\n' "$2"; FAILURES=$((FAILURES + 1)); fi; }

cleanup() {
  if [ "${RESTORE_NEEDED}" = "1" ] && [ -f "$WORK/key.jwk" ]; then
    note "==> restoring the published key"
    "$AIC" --no-prompt jwt-bearer key import "$WORK/key.jwk" --realm "$REALM" --force >/dev/null 2>&1 || true
    "$AIC" --no-prompt jwt-bearer setup --realm "$REALM" >/dev/null 2>&1 || true
  fi
  if [ "${KEEP:-0}" != "1" ]; then
    "$AIC" --no-prompt oauth delete "$CLIENT" --force --realm "$REALM" >/dev/null 2>&1 || true
  fi
  find "$WORK" -type f -exec shred -u {} + 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

[ -x "$AIC" ] || { echo "build first: cargo build" >&2; exit 1; }
if [ -n "${AGENT_PASSWORD:-}" ]; then
  printf '%s\n' "$AGENT_PASSWORD" | "$AIC" session login --password-stdin >/dev/null
fi

note "==> exporting the current private key so the tenant can be restored"
"$AIC" --no-prompt jwt-bearer key export --out "$WORK/key.jwk" >/dev/null 2>&1 \
  || { echo "no stored key; run 'aic jwt-bearer setup' first" >&2; exit 1; }
KID="$(python3 -c "import json;print(json.load(open('$WORK/key.jwk'))['kid'])")"
note "    kid $KID"

note "==> creating throwaway client with the jwt-bearer grant"
# `aic auth` sends client_secret in the form body, so the client MUST be
# client_secret_post. AM's template default is client_secret_basic, which fails
# with the same `invalid_client` string a missing key produces — seed it
# explicitly or this experiment measures client auth, not key revocation.
printf '%s' '{"advancedOAuth2ClientConfig":{"tokenEndpointAuthMethod":"client_secret_post"}}' \
  > "$WORK/seed.json"
SECRET="$("$AIC" --no-prompt oauth create "$CLIENT" --realm "$REALM" \
  --from "$WORK/seed.json" \
  --client-type Confidential --generate-secret \
  --scope openid --default-scope openid \
  --grant urn:ietf:params:oauth:grant-type:jwt-bearer 2>/dev/null \
  | sed -n 's/^client secret: //p')"
[ -n "$SECRET" ] || { echo "client creation failed" >&2; exit 1; }

mints() {
  printf '%s' "$SECRET" | "$AIC" --no-prompt auth --as-username "$SUBJECT" \
    --client-id "$CLIENT" --client-secret-stdin --scope openid --token \
    >/dev/null 2>&1
}

note "==> positive control: waiting ${SETTLE}s for client propagation"
sleep "$SETTLE"
if mints; then
  check ok "client mints before removal (positive control)"
else
  check fail "client never minted before removal — aborting, later readings would be meaningless"
  exit "$FAILURES"
fi

note "==> removing kid $KID from the published set"
RESTORE_NEEDED=1
"$AIC" --no-prompt jwt-bearer key remove "$KID" --realm "$REALM" --force >/dev/null

if "$AIC" --no-prompt jwt-bearer key list --realm "$REALM" --json 2>/dev/null \
   | grep -q "$KID"; then
  check fail "write did not land: kid still in the published jwkSet"
  exit "$FAILURES"
fi
check ok "write landed: kid absent from the published jwkSet"

note "==> polling until the removed key is refused (timeout ${TIMEOUT}s)"
START="$SECONDS"
REVOKED_AT=""
while [ $((SECONDS - START)) -lt "$TIMEOUT" ]; do
  if ! mints; then
    REVOKED_AT=$((SECONDS - START))
    break
  fi
  sleep "$INTERVAL"
  printf '    %ss: still minting\n' "$((SECONDS - START))"
done

if [ -n "$REVOKED_AT" ]; then
  note ""
  note "RESULT: removed key stopped minting after ~${REVOKED_AT}s."
  note "  Compare against the issuer's jwksCacheTimeout (default 3600000ms)."
  check ok "revocation observed"
else
  note ""
  note "RESULT: removed key STILL minting after ${TIMEOUT}s."
  note "  Revocation is slower than the window probed. Do not treat"
  note "  \`key remove\` as revocation; rotate client secrets as well."
  check ok "no revocation within the window (a result, not a harness failure)"
fi

note ""
note "update docs/api/17-jwt-bearer-user-tokens.md with whichever result this was."
exit "$FAILURES"
