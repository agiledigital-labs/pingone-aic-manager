#!/usr/bin/env bash
# verify-endpoint.sh — mint a service-account access token and curl an AIC path.
#
# Usage:
#   scripts/verify-endpoint.sh                       # mint+cache token, print nothing
#   scripts/verify-endpoint.sh /environment/variables
#   scripts/verify-endpoint.sh /am/json/realms/root/realms/alpha/scripts?_queryFilter=true \
#       --header "Accept-API-Version: protocol=2.0,resource=1.0"
#
# Env (loaded from .envrc if direnv is not active):
#   TENANT_BASE_URL, SERVICE_ACCOUNT_ID, SERVICE_ACCOUNT_KEY (JWK JSON), SERVICE_ACCOUNT_SCOPE
#
# The first run bootstraps a local Python venv with pyjwt + cryptography for JWT signing.
# The token is cached in .token-cache (gitignored) until ~60s before expiry.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
VENV="$ROOT/.venv-tools"
CACHE="$ROOT/.token-cache"

# --- Load env if not already set ---
if [ -z "${TENANT_BASE_URL:-}" ] && [ -f "$ROOT/.envrc" ]; then
  set -a
  # shellcheck disable=SC1091
  source "$ROOT/.envrc"
  set +a
fi

for var in TENANT_BASE_URL SERVICE_ACCOUNT_ID SERVICE_ACCOUNT_KEY SERVICE_ACCOUNT_SCOPE; do
  if [ -z "${!var:-}" ]; then
    echo "error: $var is not set (check .envrc)" >&2
    exit 2
  fi
done

# --- Bootstrap venv on first run ---
if [ ! -x "$VENV/bin/python3" ]; then
  echo "bootstrapping $VENV (one-time)..." >&2
  python3 -m venv "$VENV" >&2
  "$VENV/bin/pip" install -q --upgrade pip >&2
  "$VENV/bin/pip" install -q pyjwt cryptography >&2
fi

# --- Mint or reuse token ---
mint_token() {
  TENANT_BASE_URL="$TENANT_BASE_URL" \
  SERVICE_ACCOUNT_ID="$SERVICE_ACCOUNT_ID" \
  SERVICE_ACCOUNT_KEY="$SERVICE_ACCOUNT_KEY" \
  SERVICE_ACCOUNT_SCOPE="$SERVICE_ACCOUNT_SCOPE" \
  "$VENV/bin/python3" - <<'PY'
import json, os, sys, time, uuid, urllib.request, urllib.parse
import jwt
from cryptography.hazmat.primitives.asymmetric.rsa import RSAPrivateNumbers, RSAPublicNumbers
from cryptography.hazmat.primitives.serialization import Encoding, PrivateFormat, NoEncryption
from cryptography.hazmat.backends import default_backend

def b64url_decode(s):
    s += '=' * (-len(s) % 4)
    return int.from_bytes(__import__('base64').urlsafe_b64decode(s.encode()), 'big')

tenant = os.environ['TENANT_BASE_URL'].rstrip('/')
sa_id = os.environ['SERVICE_ACCOUNT_ID']
jwk = json.loads(os.environ['SERVICE_ACCOUNT_KEY'])
scope = os.environ['SERVICE_ACCOUNT_SCOPE']

# JWK RSA -> PEM
pub = RSAPublicNumbers(e=b64url_decode(jwk['e']), n=b64url_decode(jwk['n']))
priv = RSAPrivateNumbers(
    p=b64url_decode(jwk['p']), q=b64url_decode(jwk['q']),
    d=b64url_decode(jwk['d']),
    dmp1=b64url_decode(jwk['dp']), dmq1=b64url_decode(jwk['dq']),
    iqmp=b64url_decode(jwk['qi']),
    public_numbers=pub,
).private_key(backend=default_backend())
pem = priv.private_bytes(Encoding.PEM, PrivateFormat.PKCS8, NoEncryption())

# Token endpoint candidates (Q3 in plan) — try in order until one returns 200.
# Per Ping docs, the canonical endpoint is /am/oauth2/access_token at the root realm.
candidates = [
    f"{tenant}/am/oauth2/access_token",
]

now = int(time.time())
last_err = None
for token_url in candidates:
    claims = {
        'iss': sa_id,
        'sub': sa_id,
        'aud': token_url,
        'exp': now + 180,
        'jti': str(uuid.uuid4()),
    }
    assertion = jwt.encode(claims, pem, algorithm='RS256')
    body = urllib.parse.urlencode({
        'client_id': 'service-account',
        'grant_type': 'urn:ietf:params:oauth:grant-type:jwt-bearer',
        'assertion': assertion,
        'scope': scope,
    }).encode()
    req = urllib.request.Request(
        token_url, data=body,
        headers={'Content-Type': 'application/x-www-form-urlencoded'},
        method='POST',
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            resp = json.loads(r.read())
            resp['_token_url'] = token_url
            resp['_acquired_at'] = now
            print(json.dumps(resp))
            sys.exit(0)
    except urllib.error.HTTPError as e:
        last_err = (token_url, e.code, e.read().decode(errors='replace'))
    except Exception as e:
        last_err = (token_url, 'exc', str(e))

print(f"token mint failed: {last_err}", file=sys.stderr)
sys.exit(1)
PY
}

need_new=1
if [ -f "$CACHE" ]; then
  exp=$(jq -r '._acquired_at + .expires_in - 60' "$CACHE" 2>/dev/null || echo 0)
  now=$(date +%s)
  if [ "$exp" -gt "$now" ]; then
    need_new=0
  fi
fi

if [ "$need_new" -eq 1 ]; then
  out=$(mint_token)
  echo "$out" > "$CACHE"
  chmod 600 "$CACHE"
fi

TOKEN=$(jq -r '.access_token' "$CACHE")
TOKEN_URL=$(jq -r '._token_url' "$CACHE")

# --- If no path given, just print token info (sans secret) ---
if [ "$#" -eq 0 ]; then
  jq '{token_url: ._token_url, expires_in, scope, token_type, acquired_at: ._acquired_at}' "$CACHE"
  exit 0
fi

PATH_ARG="$1"
shift

# --- Default Accept-API-Version unless caller provided one ---
have_apiver=0
for a in "$@"; do
  case "$a" in
    *"Accept-API-Version"*) have_apiver=1 ;;
  esac
done

extra_headers=()
if [ "$have_apiver" -eq 0 ]; then
  extra_headers+=(--header "Accept-API-Version: resource=1.0")
fi

URL="${TENANT_BASE_URL%/}${PATH_ARG}"
echo "GET $URL" >&2
echo "  token from: $TOKEN_URL" >&2

curl -sS -o /tmp/aic-verify.body -w "HTTP %{http_code}\n" \
  --header "Authorization: Bearer $TOKEN" \
  --header "Accept: application/json" \
  "${extra_headers[@]}" \
  "$@" \
  "$URL" >&2

if jq -e . /tmp/aic-verify.body >/dev/null 2>&1; then
  jq . /tmp/aic-verify.body
else
  cat /tmp/aic-verify.body
fi
