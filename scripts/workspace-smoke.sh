#!/usr/bin/env bash
# Workspace smoke test: generate a fresh workspace, drop in known-clean sample
# scripts for each family, then run `npm run lint` and `npm run type-check` to
# prove the generated toolchain actually works end-to-end.
#
# Requires network access (npm install). Builds the aic binary if needed.
# The throwaway workspace is created under workspace/<tenant>/ (gitignored) and
# removed on exit.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
TENANT="${SMOKE_TENANT:-smoke-$$}"
WS="workspace/$TENANT"
AIC_BIN="${AIC_BIN:-$ROOT/target/debug/aic}"

cleanup() { rm -rf "$WS"; }
trap cleanup EXIT

echo "==> building aic"
cargo build --offline -q

# `workspace init` generates managed/sync type files from the tenant, so it
# needs an unlocked agent even though the rest of this test is local. Unlock
# non-interactively when AGENT_PASSWORD is exported (direnv does this from
# .envrc); the call is idempotent on an already-unlocked agent. Fail loudly
# rather than letting the run die at the first step with an opaque message.
if [ -n "${AGENT_PASSWORD:-}" ]; then
  echo "==> unlocking agent"
  printf '%s\n' "$AGENT_PASSWORD" | "$AIC_BIN" session login --password-stdin >/dev/null
elif ! "$AIC_BIN" --no-prompt whoami --token >/dev/null 2>&1; then
  echo "error: agent is locked and AGENT_PASSWORD is not set." >&2
  echo "  run 'aic login', or export AGENT_PASSWORD, then re-run." >&2
  exit 1
fi

echo "==> aic workspace init --tenant $TENANT"
"$AIC_BIN" workspace init --tenant "$TENANT" >/dev/null

echo "==> writing known-clean sample scripts + leaf tsconfigs"
mkdir -p "$WS/am/alpha/decision-node" "$WS/am/alpha/lib" "$WS/idm/endpoint" \
  "$WS/am/alpha/oauth2-access-token-ng" "$WS/am/alpha/oidc-claims"

# Leaf tsconfigs mirror am::leaf_tsconfig (whose output is covered by the Rust
# unit tests); a real pull writes these automatically.
cat > "$WS/am/alpha/decision-node/tsconfig.json" <<'JSON'
{
  "extends": "../../tsconfig.json",
  "include": ["./**/*", "../../types/rhino-1.7.14.d.ts", "../../types/common.d.ts", "../../types/nextgen-common.d.ts", "../../types/decision-node-base.d.ts", "../../types/decision-node-next.d.ts"],
  "compilerOptions": { "paths": { "*": ["../lib/*"] } }
}
JSON
cat > "$WS/am/alpha/lib/tsconfig.json" <<'JSON'
{
  "extends": "../../tsconfig.json",
  "include": ["./**/*", "../../types/rhino-1.7.14.d.ts", "../../types/common.d.ts", "../../types/nextgen-common.d.ts", "../../types/library-args.d.ts", "../../types/library.d.ts"],
  "compilerOptions": { "paths": { "*": ["./*"] } }
}
JSON

cat > "$WS/am/alpha/decision-node/Sample.cjs" <<'JS'
var name = nodeState.get("username");
logger.info("hello {}", name);
var lib = require("MyLib");
lib.prompt(callbacksBuilder, "who are you?");
action.goTo(lib.ok ? "true" : "false");
JS
# The factory annotates its parameters, which is the whole point of
# library-args.d.ts: a library sees no `callbacksBuilder` binding, but has to be
# able to name its type to accept one.
cat > "$WS/am/alpha/lib/MyLib.cjs" <<'JS'
const ok = true;

/**
 * @param {CallbacksBuilder} callbacksBuilder
 * @param {string} message
 */
function prompt(callbacksBuilder, message) {
  callbacksBuilder.textOutputCallback(0, message);
}

exports.ok = ok;
exports.prompt = prompt;
JS
cat > "$WS/am/alpha/lib/MyLib.js" <<'JS'
export * from "./MyLib.cjs";
JS
cat > "$WS/am/alpha/oauth2-access-token-ng/tsconfig.json" <<'JSON'
{
  "extends": "../../tsconfig.json",
  "include": ["./**/*", "../../types/rhino-1.7.14.d.ts", "../../types/common.d.ts", "../../types/nextgen-common.d.ts", "../../types/oauth2-access-token-ng.d.ts", "../../types/managed/*.d.ts"],
  "compilerOptions": { "paths": { "*": ["../lib/*"] } }
}
JSON
cat > "$WS/am/alpha/oidc-claims/tsconfig.json" <<'JSON'
{
  "extends": "../../tsconfig.json",
  "include": ["./**/*", "../../types/rhino-1.7.14.d.ts", "../../types/oidc-claims.d.ts"]
}
JSON

# `requestProperties`/`clientProperties` are an unenumerated `object` in the
# editor metadata; this is the read that proves they are named types instead.
# The header is bound and guarded because indexing straight into the map is both
# a type error and a Rhino TypeError on a request that lacks it.
cat > "$WS/am/alpha/oauth2-access-token-ng/Sample.cjs" <<'JS'
function firstHeader(name) {
  var v = requestProperties.requestHeaders[name];
  return v && v.length ? String(v[0]) : null;
}
logger.info("{} {} {}", firstHeader("content-type"),
  requestProperties.requestUri, clientProperties.clientId);
if (clientProperties.allowedScopes.contains("openid")) {
  accessToken.setField("aicedit_smoke", true);
}
JS

# Legacy OIDC claims: every one of these reaches a Java collection with a JS
# string literal, which is what the `Lookup` widening in rhino-1.7.14.d.ts is for.
cat > "$WS/am/alpha/oidc-claims/Sample.cjs" <<'JS'
var ct = requestProperties.requestHeaders.get("content-type");
var wanted = requestedClaims.get("email");
var attr = identity.getAttribute("mail");
logger.message(
  "" + ct + wanted + attr + scopes.contains("openid") +
  claimsLocales.includes("en") +
  clientProperties.allowedGrantTypes.contains("authorization_code") +
  session.getProperty("Principal")
);
JS

cat > "$WS/idm/endpoint/myEndpoint.cjs" <<'JS'
var users = openidm.query("managed/alpha_user", { _queryFilter: "true" });
logger.info("endpoint {} found {}", request.method, users);

// context.oauth2 is optional (absent for schedules and internal callers) and
// its scopes are a java.util.Set, so membership is contains(), not includes().
if (!context.oauth2 || !context.oauth2.scopes.contains("fr:idm:*")) {
  throw { code: 403, message: "Missing required OAuth scope" };
}
logger.info("scope count {}", context.oauth2.scopes.size());
JS

echo "==> npm install"
( cd "$WS" && npm install --no-audit --no-fund --silent )

echo "==> npm run lint"
( cd "$WS" && npm run lint )

echo "==> npm run type-check"
( cd "$WS" && npm run type-check )

echo "SMOKE OK"
