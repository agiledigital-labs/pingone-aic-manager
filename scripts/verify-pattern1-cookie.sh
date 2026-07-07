#!/usr/bin/env bash
# verify-pattern1-cookie.sh — verify the "paste session cookie" bootstrap flow.
#
# Takes an AM session cookie value (from the user's logged-in browser tab),
# exchanges it for an access token via idmAdminClient PKCE, creates a throwaway
# service account, mints a token using the SA's own JWK to prove the round-trip,
# then deletes the SA. Leaves no trace if all steps succeed.
#
# Usage:
#   scripts/verify-pattern1-cookie.sh <session-cookie-value>
#   scripts/verify-pattern1-cookie.sh -        # read value from stdin
#
# Env:
#   TENANT_BASE_URL — required (loaded from .envrc if direnv inactive)

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
PY="$ROOT/.venv-tools/bin/python3"

if [ -z "${TENANT_BASE_URL:-}" ] && [ -f "$ROOT/.envrc" ]; then
  set -a; source "$ROOT/.envrc"; set +a
fi
: "${TENANT_BASE_URL:?TENANT_BASE_URL not set (check .envrc)}"
[ -x "$PY" ] || { echo "error: .venv-tools missing. Run scripts/verify-endpoint.sh once to bootstrap." >&2; exit 2; }

if [ $# -lt 1 ]; then
  echo "usage: $0 <session-cookie-value | ->" >&2
  exit 2
fi
if [ "$1" = "-" ]; then
  read -r SESSION
else
  SESSION="$1"
fi
[ -n "$SESSION" ] || { echo "error: empty session cookie" >&2; exit 2; }

TENANT="${TENANT_BASE_URL#https://}"
TENANT="${TENANT%/}"
REDIRECT_URI="https://$TENANT/platform/appAuthHelperRedirect.html"

step() { printf '\n\033[1;36m== %s ==\033[0m\n' "$*"; }
ok()   { printf '\033[32m  ok\033[0m  %s\n' "$*"; }
fail() { printf '\033[31m  FAIL\033[0m  %s\n' "$*" >&2; exit 1; }

step "1. Discover AM cookie name"
COOKIE_NAME=$(curl -fsS "https://$TENANT/am/json/serverinfo/*" \
  | $PY -c "import sys,json; print(json.load(sys.stdin)['cookieName'])")
ok "cookie name: $COOKIE_NAME"

step "2. authorize idmAdminClient (PKCE) with session cookie"
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
  -H "Cookie: $COOKIE_NAME=$SESSION; amlbcookie=01" \
  | grep -i '^location:' | head -1 | sed 's/^[Ll]ocation: //; s/\r$//')
CODE=$(printf '%s' "$LOC" | grep -o 'code=[^&]*' | sed 's/code=//' || true)
[ -n "$CODE" ] || fail "no code in Location header: $LOC"
ok "got code"

step "3. exchange code for access token"
TOKEN_JSON=$(curl -fsS -X POST "https://$TENANT/am/oauth2/realms/root/access_token" \
  --data-urlencode "grant_type=authorization_code" \
  --data-urlencode "code=$CODE" \
  --data-urlencode "client_id=idmAdminClient" \
  --data-urlencode "redirect_uri=$REDIRECT_URI" \
  --data-urlencode "code_verifier=$V")
AT=$(echo "$TOKEN_JSON" | $PY -c "import sys,json; print(json.load(sys.stdin)['access_token'])")
SCOPES=$(echo "$TOKEN_JSON" | $PY -c "import sys,json; print(json.load(sys.stdin).get('scope',''))")
ok "bearer length=${#AT}, scope=$SCOPES"

step "4. generate RSA-2048 keypair + public JWKS"
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

step "5. create throwaway service account"
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

step "6. SA mints its own access_token via JWT-bearer"
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

step "7. delete throwaway service account"
DEL_STATUS=$(curl -sS -o /dev/null -w '%{http_code}' -X DELETE \
  "https://$TENANT/openidm/managed/svcacct/$SA_ID" \
  -H "Authorization: Bearer $AT")
[ "$DEL_STATUS" = "200" ] || fail "DELETE returned $DEL_STATUS"
ok "deleted"

step "8. confirm SA gone"
GET_STATUS=$(curl -sS -o /dev/null -w '%{http_code}' \
  "https://$TENANT/openidm/managed/svcacct/$SA_ID" \
  -H "Authorization: Bearer $AT")
[ "$GET_STATUS" = "404" ] || fail "expected 404 after delete, got $GET_STATUS"
ok "404 confirmed"

printf '\n\033[1;32mPattern 1 verified end-to-end.\033[0m\n'
