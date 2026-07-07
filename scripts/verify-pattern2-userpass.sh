#!/usr/bin/env bash
# verify-pattern2-userpass.sh — verify the "username/password in-app" bootstrap.
#
# Authenticates a user against AM's Login journey (no MFA or TOTP only — passkey
# flows can't be driven from a TTY), captures the tokenId, then runs the same
# OAuth2 + service-account round-trip as pattern 1 to prove it works end-to-end.
#
# Usage:
#   scripts/verify-pattern2-userpass.sh                      # prompts for u/p
#   scripts/verify-pattern2-userpass.sh -u <user>            # prompts only for pw
#   scripts/verify-pattern2-userpass.sh -u <user> -r <realm> # realm defaults to root
#   scripts/verify-pattern2-userpass.sh -u <user> -t <tree>  # tree defaults to Login
#
# Env: TENANT_BASE_URL (from .envrc).
# Stdin: any additional callbacks (e.g. TOTP code) are read interactively.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
PY="$ROOT/.venv-tools/bin/python3"

if [ -z "${TENANT_BASE_URL:-}" ] && [ -f "$ROOT/.envrc" ]; then
  set -a; source "$ROOT/.envrc"; set +a
fi
: "${TENANT_BASE_URL:?TENANT_BASE_URL not set (check .envrc)}"
[ -x "$PY" ] || { echo "error: .venv-tools missing. Run scripts/verify-endpoint.sh once to bootstrap." >&2; exit 2; }

USER=""
REALM="root"
TREE=""   # empty → hit realm default journey
while [ $# -gt 0 ]; do
  case "$1" in
    -u) USER="$2"; shift 2;;
    -r) REALM="$2"; shift 2;;
    -t) TREE="$2"; shift 2;;
    -h|--help) sed -n '1,15p' "$0"; exit 0;;
    *) echo "unknown arg: $1" >&2; exit 2;;
  esac
done

TENANT="${TENANT_BASE_URL#https://}"
TENANT="${TENANT%/}"
REDIRECT_URI="https://$TENANT/platform/appAuthHelperRedirect.html"
REALM_PATH="/realms/root"
if [ "$REALM" != "root" ]; then REALM_PATH="/realms/root/realms/$REALM"; fi
AUTH_QS=""
if [ -n "$TREE" ]; then AUTH_QS="?authIndexType=service&authIndexValue=$TREE"; fi

step() { printf '\n\033[1;36m== %s ==\033[0m\n' "$*"; }
ok()   { printf '\033[32m  ok\033[0m  %s\n' "$*"; }
fail() { printf '\033[31m  FAIL\033[0m  %s\n' "$*" >&2; exit 1; }

if [ -z "$USER" ]; then
  read -rp "username: " USER
fi
read -rsp "password: " PASS; echo

step "1. start AM authentication (${TREE:-default journey} in $REALM realm)"
AUTH_URL="https://$TENANT/am/json$REALM_PATH/authenticate$AUTH_QS"
RESP=$(curl -fsS -X POST "$AUTH_URL" \
  -H "Accept-API-Version: resource=2.0, protocol=1.0" \
  -H "Content-Type: application/json") || fail "authenticate endpoint not reachable"
ok "initial callbacks received"

# Walk callbacks: fill Name/Password from args, prompt for everything else.
ITER=0
while :; do
  ITER=$((ITER+1))
  [ $ITER -gt 6 ] && fail "too many callback rounds — journey too complex for this script"

  TOKEN_ID=$(echo "$RESP" | $PY -c "import sys,json;print(json.load(sys.stdin).get('tokenId',''))" 2>/dev/null || echo "")
  if [ -n "$TOKEN_ID" ]; then
    ok "tokenId obtained (round $((ITER-1)))"
    break
  fi

  HAS_CB=$(echo "$RESP" | $PY -c "import sys,json; d=json.load(sys.stdin); print(1 if d.get('callbacks') else 0)" 2>/dev/null || echo 0)
  [ "$HAS_CB" = "1" ] || fail "no tokenId and no callbacks: $RESP"

  STAGE=$(echo "$RESP" | $PY -c "import sys,json;print(json.load(sys.stdin).get('stage','?'))")
  echo "  round $ITER (stage=$STAGE)"

  RESP=$(USER="$USER" PASS="$PASS" CB_JSON="$RESP" $PY <<'PYEOF'
import sys, os, json
d = json.loads(os.environ["CB_JSON"])
# Python's stdin is consumed by the heredoc; talk to the user via /dev/tty.
try:
    tty_r = open("/dev/tty", "r"); tty_w = open("/dev/tty", "w")
except OSError:
    tty_r, tty_w = sys.stdin, sys.stderr
def ask(prompt):
    tty_w.write(prompt); tty_w.flush()
    return tty_r.readline().rstrip("\n")

def output(cb, name, default=None):
    for k in cb.get("output", []):
        if k.get("name") == name: return k.get("value", default)
    return default

OTP_HINTS = ("otp", "code", "token", "verification", "verify")
USERNAME_HINTS = ("user", "name", "email", "login")
for cb in d.get("callbacks", []):
    t = cb.get("type", "")
    inp = cb.get("input", [])
    prompt = output(cb, "prompt", "") or ""
    p = prompt.lower()
    if t == "NameCallback":
        # AIC reuses NameCallback for OTPs (HAR shows prompt='Enter verification code').
        # Username if prompt mentions name/user/email, OTP otherwise.
        if any(w in p for w in USERNAME_HINTS) and not any(w in p for w in OTP_HINTS):
            inp[0]["value"] = os.environ["USER"]
        elif any(w in p for w in OTP_HINTS):
            inp[0]["value"] = ask(f"  {prompt}: ")
        else:
            inp[0]["value"] = ask(f"  callback '{prompt or t}': ")
    elif t == "PasswordCallback":
        if any(w in p for w in OTP_HINTS):
            inp[0]["value"] = ask(f"  {prompt}: ")
        else:
            inp[0]["value"] = os.environ["PASS"]
    elif t == "ConfirmationCallback":
        # The hidden "Submit" button: send defaultOption (usually 0). No user prompt.
        inp[0]["value"] = int(output(cb, "defaultOption", 0))
    elif t in ("StringAttributeInputCallback", "TextInputCallback"):
        inp[0]["value"] = ask(f"  {prompt or t}: ")
    elif t == "BooleanAttributeInputCallback":
        inp[0]["value"] = False  # safe default for "Trust this device" etc.
    elif t in ("TextOutputCallback", "HiddenValueCallback"):
        pass
    elif t == "PollingWaitCallback":
        sys.stderr.write(f"  polling required ({prompt}) — passkey/push flows not supported by this script\n")
        sys.exit(3)
    else:
        sys.stderr.write(f"  unhandled callback type {t} (prompt={prompt}) — extend the script\n")
        sys.exit(3)
print(json.dumps(d))
PYEOF
  ) || fail "callback handling failed (passkey/push not supported here)"

  RESP=$(curl -fsS -X POST "$AUTH_URL" \
    -H "Accept-API-Version: resource=2.0, protocol=1.0" \
    -H "Content-Type: application/json" \
    -d "$RESP") || fail "authenticate POST failed at round $ITER"
done

step "2. discover AM cookie name"
COOKIE_NAME=$(curl -fsS "https://$TENANT/am/json/serverinfo/*" \
  | $PY -c "import sys,json; print(json.load(sys.stdin)['cookieName'])")
ok "cookie name: $COOKIE_NAME"

step "3. authorize idmAdminClient (PKCE) using tokenId as session cookie"
V=$(openssl rand -base64 32 | tr -d '\n=' | tr '/+' '_-')
C=$(printf '%s' "$V" | openssl dgst -binary -sha256 | base64 | tr -d '\n=' | tr '/+' '_-')
LOC=$(curl -sS -i -G "https://$TENANT/am/oauth2/realms/root/authorize" \
  --data-urlencode "client_id=idmAdminClient" \
  --data-urlencode "response_type=code" \
  --data-urlencode "scope=openid fr:idm:*" \
  --data-urlencode "redirect_uri=$REDIRECT_URI" \
  --data-urlencode "code_challenge=$C" \
  --data-urlencode "code_challenge_method=S256" \
  --data-urlencode "state=verify" \
  -H "Cookie: $COOKIE_NAME=$TOKEN_ID; amlbcookie=01" \
  | grep -i '^location:' | head -1 | sed 's/^[Ll]ocation: //; s/\r$//')
CODE=$(printf '%s' "$LOC" | grep -o 'code=[^&]*' | sed 's/code=//' || true)
[ -n "$CODE" ] || fail "no code: $LOC"
ok "got code"

step "4. exchange code for access token"
TOKEN_JSON=$(curl -fsS -X POST "https://$TENANT/am/oauth2/realms/root/access_token" \
  --data-urlencode "grant_type=authorization_code" \
  --data-urlencode "code=$CODE" \
  --data-urlencode "client_id=idmAdminClient" \
  --data-urlencode "redirect_uri=$REDIRECT_URI" \
  --data-urlencode "code_verifier=$V")
AT=$(echo "$TOKEN_JSON" | $PY -c "import sys,json; print(json.load(sys.stdin)['access_token'])")
SCOPES=$(echo "$TOKEN_JSON" | $PY -c "import sys,json; print(json.load(sys.stdin).get('scope',''))")
ok "bearer length=${#AT}, scope=$SCOPES"

step "5. generate RSA-2048 keypair + public JWKS"
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT
openssl genrsa -out "$TMPDIR/sa.pem" 2048 2>/dev/null
KID="pingone-aic-manager-verify-$(date +%s)"
PUB_JWKS=$($PY - "$TMPDIR/sa.pem" "$KID" <<'PYEOF'
import json, base64, sys
from cryptography.hazmat.primitives import serialization
with open(sys.argv[1],"rb") as f: key = serialization.load_pem_private_key(f.read(), password=None)
pub = key.public_key().public_numbers()
b = lambda x: base64.urlsafe_b64encode(x.to_bytes((x.bit_length()+7)//8,'big')).rstrip(b'=').decode()
print(json.dumps({"keys":[{"kty":"RSA","kid":sys.argv[2],"alg":"RS256","use":"sig","n":b(pub.n),"e":b(pub.e)}]}))
PYEOF
)
ok "kid=$KID"

step "6. create throwaway service account"
CREATE_BODY=$($PY - "$PUB_JWKS" <<'PYEOF'
import json, sys
print(json.dumps({
  "name": "pingone-aic-manager-verify-DELETEME",
  "description": "temporary SA — delete on failure",
  "scopes": ["fr:idm:*","fr:am:*","fr:idc:esv:*","fr:idc:cookie-domain:*"],
  "accountStatus": "Active",
  "jwks": sys.argv[1],
}))
PYEOF
)
CREATE_RESP=$(curl -fsS -X POST "https://$TENANT/openidm/managed/svcacct?_action=create" \
  -H "Authorization: Bearer $AT" \
  -H "Content-Type: application/json" \
  -d "$CREATE_BODY")
SA_ID=$(echo "$CREATE_RESP" | $PY -c "import sys,json; print(json.load(sys.stdin)['_id'])")
ok "SA created: $SA_ID"

step "7. SA mints its own access_token via JWT-bearer"
ASSERTION=$($PY - "$TMPDIR/sa.pem" "$KID" "$SA_ID" "$TENANT" <<'PYEOF'
import sys, time, jwt
with open(sys.argv[1],"rb") as f: pem = f.read()
now = int(time.time())
tok = jwt.encode(
  {"iss": sys.argv[3], "sub": sys.argv[3],
   "aud": f"https://{sys.argv[4]}/am/oauth2/access_token",
   "exp": now + 60, "jti": f"verify-{now}"},
  pem, algorithm="RS256", headers={"kid": sys.argv[2]})
print(tok)
PYEOF
)
SA_TOK_RESP=$(curl -fsS -X POST "https://$TENANT/am/oauth2/access_token" \
  --data-urlencode "client_id=service-account" \
  --data-urlencode "grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer" \
  --data-urlencode "scope=fr:idm:* fr:am:* fr:idc:esv:*" \
  --data-urlencode "assertion=$ASSERTION")
SA_AT=$(echo "$SA_TOK_RESP" | $PY -c "import sys,json; print(json.load(sys.stdin)['access_token'])")
SA_SCOPE=$(echo "$SA_TOK_RESP" | $PY -c "import sys,json; print(json.load(sys.stdin).get('scope',''))")
ok "SA bearer length=${#SA_AT}, scope=$SA_SCOPE"

step "8. delete throwaway service account"
DEL_STATUS=$(curl -sS -o /dev/null -w '%{http_code}' -X DELETE \
  "https://$TENANT/openidm/managed/svcacct/$SA_ID" \
  -H "Authorization: Bearer $AT")
[ "$DEL_STATUS" = "200" ] || fail "DELETE returned $DEL_STATUS"
ok "deleted"

step "9. confirm SA gone"
GET_STATUS=$(curl -sS -o /dev/null -w '%{http_code}' \
  "https://$TENANT/openidm/managed/svcacct/$SA_ID" \
  -H "Authorization: Bearer $AT")
[ "$GET_STATUS" = "404" ] || fail "expected 404 after delete, got $GET_STATUS"
ok "404 confirmed"

printf '\n\033[1;32mPattern 2 verified end-to-end.\033[0m\n'
