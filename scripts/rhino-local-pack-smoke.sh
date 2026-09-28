#!/usr/bin/env bash
# Install the packed rhino-local into a throwaway consumer and use it there
# (scripts/rhino-local-pack-smoke.sh).
#
# Usage:
#   scripts/rhino-local-pack-smoke.sh
#
# The repo's own suite imports the harness from source, so it cannot notice a
# package that only works beside this checkout: a file missing from `files`, a
# path resolved through the repo, a declaration that does not type-check, a
# runner that still needs `javac`. This packs the package, installs the tarball
# into a directory outside the repo, and there:
#
#   1. type-checks a consumer test against the shipped declarations,
#      skipLibCheck off, without allowImportingTsExtensions;
#   2. runs it with a Java home holding `java` and no `javac`, a fresh cache,
#      and the jar supplied offline through RHINO_LOCAL_RHINO_JAR — so the
#      prebuilt classes are the only way the runner can start;
#   3. checks nothing was compiled, and that state landed in the consumer's
#      own self-ignoring `.rhino-local/`, not in node_modules;
#   4. runs the fetch-jar bin.
#
# Needs a JDK 25 to build (nix-shell provides one) and npm registry access, or
# a warm npm cache, for the consumer's vitest/zod/typescript.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
PKG="$ROOT/packages/rhino-local"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/rhino-local-smoke.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

fail() {
  echo "rhino-local-pack-smoke: $*" >&2
  exit 1
}

java_home="${RHINO_LOCAL_JAVA_HOME:-${JAVA_HOME:-}}"
if [[ -n "$java_home" ]]; then
  java_bin="$java_home/bin/java"
else
  java_bin="$(command -v java)" || fail "no java on PATH; set JAVA_HOME"
fi

(cd "$PKG" && npm run build >/dev/null && npm pack --pack-destination "$WORK" >/dev/null 2>&1)
tarball="$(find "$WORK" -maxdepth 1 -name '*.tgz' -print -quit)"
[[ -n "$tarball" ]] || fail "npm pack produced no tarball"
name="$(node -p 'require(process.argv[1]).name' "$PKG/package.json")"
dev() { node -p 'require(process.argv[1]).devDependencies[process.argv[2]]' "$PKG/package.json" "$1"; }

# The jar, from wherever this machine keeps it, so the consumer runs offline.
jar="$(cd "$PKG" && node bin/fetch-jar.ts)"

consumer="$WORK/consumer"
mkdir -p "$consumer/test"
# A repository of its own, as a real consumer is: it makes the consumer the
# project root, and lets the end ask git whether the state would be committed.
git init -q "$consumer"
cat >"$consumer/package.json" <<JSON
{ "name": "rhino-local-consumer", "private": true, "type": "module" }
JSON
cat >"$consumer/tsconfig.json" <<'JSON'
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "noEmit": true,
    "skipLibCheck": false,
    "exactOptionalPropertyTypes": true,
    "types": ["node"]
  },
  "include": ["test/**/*.ts"]
}
JSON
cat >"$consumer/test/smoke.test.ts" <<TS
import { describe, expect, it } from "vitest";
import { z } from "zod";
import { defineSuite, useLease } from "$name";
import { validateCase } from "$name/case";
import { tokenCallbackProvider } from "$name/aic";

const suite = defineSuite({
  name: "consumer-smoke",
  script: [
    'if (nodeState.get("userId") === "alice") {',
    '  nodeState.putShared("greeted", true);',
    '  action.goTo("known");',
    '} else { action.goTo("unknown"); }',
  ].join("\\n"),
  outcomes: ["known", "unknown"],
  inputs: z.object({ userId: z.string() }),
});

describe("installed rhino-local", () => {
  const lease = useLease(suite, { timeoutMs: 30_000 });

  it("runs a script on the host JVM", async () => {
    const run = await lease
      .run({ userId: "alice" })
      .expect({ outcome: "known", sharedState: { added: { greeted: true } } });
    expect(run.verdict.summary, run.verdict.summary).toBe("");
  });

  it("judges a wrong expectation as a failure", async () => {
    const run = await lease.run({ userId: "bob" }).expect({ outcome: "known" });
    expect(run.verdict.pass).toBe(false);
  });

  // Positive control for step 1: were the declarations to resolve to \`any\`,
  // this directive would be unused and tsc would fail.
  it("types the inputs from the suite's schema", () => {
    const wrong = () =>
      // @ts-expect-error userId is required by the suite's inputs
      lease.run({}).expect({ outcome: "known" });
    expect(typeof wrong).toBe("function");
  });

  it("exports the case and provider surfaces", () => {
    expect(typeof validateCase).toBe("function");
    expect(() => tokenCallbackProvider({ baseUrl: "http://tenant.example.com", getToken: async () => "" })).toThrow(/https/);
  });
});
TS

(
  cd "$consumer"
  npm install --prefer-offline --no-audit --no-fund --loglevel=error \
    "$tarball" "vitest@$(dev vitest)" "zod@$(dev zod)" \
    "typescript@$(dev typescript)" "@types/node@$(dev @types/node)" >/dev/null
)
installed="$consumer/node_modules/$name"
[[ -d "$installed/dist/classes" ]] || fail "the tarball ships no prebuilt classes"
[[ ! -e "$installed/test" ]] || fail "the tarball ships the test suite"

(cd "$consumer" && npx tsc -p .) || fail "the consumer does not type-check against the shipped declarations"

jre="$WORK/jre"
mkdir -p "$jre/bin"
ln -s "$java_bin" "$jre/bin/java"
cache="$WORK/cache"
(
  cd "$consumer"
  env -u JAVA_HOME RHINO_LOCAL_JAVA_HOME="$jre" RHINO_LOCAL_CACHE="$cache" \
    RHINO_LOCAL_RHINO_JAR="$jar" CI=1 npx vitest run
) || fail "the consumer's tests failed"

[[ ! -e "$cache/classes" ]] || fail "the runner compiled classes instead of using the shipped ones"
# The suite above runs the local lane, which writes no state. So write a
# failure record through the installed module itself, from a subdirectory, and
# check it lands in the consumer's own state directory and ignores itself.
(
  cd "$consumer/test"
  node --input-type=module -e '
    const { appendFailure } = await import(process.argv[1]);
    await appendFailure({ testName: "t", stem: "s", passIds: ["p"], file: "f",
      suite: "x", timestamp: "2026-01-01T00:00:00.000Z", tenant: "tenant" });
  ' "$installed/dist/src/harness/failures.js"
)
state="$consumer/.rhino-local"
[[ -f "$state/failures/failures.jsonl" ]] || fail "no failure record in $state"
[[ "$(cat "$state/.gitignore")" == "*" ]] || fail "$state does not ignore itself"
[[ "$(stat -c %a "$state/failures/failures.jsonl")" == 600 ]] || fail "the failure record is not 0600"
[[ -z "$(git -C "$consumer" status --porcelain -- .rhino-local)" ]] || fail "git would commit $state"
[[ ! -e "$installed/.rhino-local" ]] || fail "state was written into node_modules"

fetched="$(cd "$consumer" && RHINO_LOCAL_CACHE="$cache" RHINO_LOCAL_RHINO_JAR="$jar" npx rhino-local-fetch-jar)"
[[ "$fetched" == "$jar" ]] || fail "rhino-local-fetch-jar printed $fetched, expected $jar"

echo "rhino-local-pack-smoke: ok ($(basename "$tarball"))"
