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

echo "==> aic script workspace init --tenant $TENANT"
"$AIC_BIN" script workspace init --tenant "$TENANT" >/dev/null

echo "==> writing known-clean sample scripts + leaf tsconfigs"
mkdir -p "$WS/am/alpha/decision-node" "$WS/am/alpha/lib" "$WS/idm/endpoint"

# Leaf tsconfigs mirror am::leaf_tsconfig (whose output is covered by the Rust
# unit tests); a real pull writes these automatically.
cat > "$WS/am/alpha/decision-node/tsconfig.json" <<'JSON'
{
  "extends": "../../tsconfig.json",
  "include": ["./**/*", "../../types/rhino-1.7.14.d.ts", "../../types/common.d.ts", "../../types/decision-node-base.d.ts", "../../types/decision-node-next.d.ts"],
  "compilerOptions": { "paths": { "*": ["../lib/*"] } }
}
JSON
cat > "$WS/am/alpha/lib/tsconfig.json" <<'JSON'
{
  "extends": "../../tsconfig.json",
  "include": ["./**/*", "../../types/rhino-1.7.14.d.ts", "../../types/common.d.ts", "../../types/library.d.ts"],
  "compilerOptions": { "paths": { "*": ["./*"] } }
}
JSON

cat > "$WS/am/alpha/decision-node/Sample.cjs" <<'JS'
var name = nodeState.get("username");
logger.info("hello {}", name);
var lib = require("MyLib");
action.goTo(lib.ok ? "true" : "false");
JS
cat > "$WS/am/alpha/lib/MyLib.cjs" <<'JS'
var ok = true;
exports.ok = ok;
JS
cat > "$WS/am/alpha/lib/MyLib.js" <<'JS'
export * from "./MyLib.cjs";
JS
cat > "$WS/idm/endpoint/myEndpoint.cjs" <<'JS'
var users = openidm.query("managed/alpha_user", { _queryFilter: "true" });
logger.info("endpoint {} found {}", request.method, users);
JS

echo "==> npm install"
( cd "$WS" && npm install --no-audit --no-fund --silent )

echo "==> npm run lint"
( cd "$WS" && npm run lint )

echo "==> npm run type-check"
( cd "$WS" && npm run type-check )

echo "SMOKE OK"
