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
#      and the jar supplied offline through AIC_SCRIPT_TESTER_RHINO_JAR — so the
#      prebuilt classes are the only way the runner can start;
#   3. checks nothing was compiled, and that state landed in the consumer's
#      own self-ignoring `.aic-script-tester/`, not in node_modules;
#   4. runs the fetch-jar bin;
#   5. exercises the canonical identity-policy asset from the installed build,
#      including Node validation and Rhino collision preflight.
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

java_home="${AIC_SCRIPT_TESTER_JAVA_HOME:-${JAVA_HOME:-}}"
if [[ -n "$java_home" ]]; then
  java_bin="$java_home/bin/java"
else
  java_bin="$(command -v java)" || fail "no java on PATH; set JAVA_HOME"
fi

# `prepack` builds dist/ and the prebuilt classes, so this is also the check
# that a bare `npm publish` cannot ship a package without them.
(cd "$PKG" && rm -rf dist && npm pack --pack-destination "$WORK" >/dev/null 2>&1)
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
# Exercise the shipped worker/reporter entries and documented runner subclass.
cat >"$consumer/test/client-runner.ts" <<TS
import EmulationRunner from "$name/vitest-runner";
import type { RunnerTask } from "vitest";
export default class ClientRunner extends EmulationRunner {
  override onAfterRunTask(task: RunnerTask): void {
    super.onAfterRunTask(task);
  }
}
TS
cat >"$consumer/vitest.config.ts" <<TS
import { defineConfig } from "vitest/config";
export default defineConfig({ test: {
  setupFiles: ["$name/vitest-setup"],
  runner: "./test/client-runner.ts",
  reporters: ["default", "$name/vitest-reporter"],
} });
TS
cat >"$consumer/test/smoke.test.ts" <<TS
import { describe, expect, it } from "vitest";
import { z } from "zod";
import { defineSuite, expectHarnessFault, useLease } from "$name";
import { defineCase, validateCase } from "$name/case";
import { RhinoRunner } from "$name/runner";
import { mockPreamble, withHarvest, parseHarvest, runCase } from "$name/bindings";
import { tokenCallbackProvider } from "$name/aic";
import { readFailures, runShowLog } from "$name/diagnostics";
import { profilePath } from "$name/profile";

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

  // The policy asset must load from the installed package in both engines.
  it("shares collision policy between compiled Node and Rhino", async () => {
    const resource = "managed/alpha_user/example";
    const managed = { "managed/alpha_user": [{ _id: "example" }] };
    const seed = { managed, identityCustomAttrs: { [resource]: [] } };
    expect(() => validateCase({
      name: "collision", script: 'action.goTo("true");', outcomes: ["true"],
      given: { ...seed, identityCustomAttrsOwnedKeys: { [resource]: ["givenName"] } },
      expect: { outcome: "true" },
    })).toThrow(/collides with/);
    const runner = await RhinoRunner.spawn();
    try {
      expectHarnessFault("identity-bag-key-collision");
      const response = await runner.eval({
        preamble: mockPreamble({ ...seed, identityCustomAttrsOwnedKeys: { [resource]: ["plain"] } }),
        source: withHarvest([
          '__rhinoLocal.identityCustomAttrsOwnedKeys["managed/alpha_user/example"].push("givenName");',
          'try { openidm.patch("managed/alpha_user/example", null, [{operation:"add",field:"plain",value:"partial"},{operation:"add",field:"givenName",value:"invalid"}]); }',
          'catch(e) { nodeState.putShared("error", String(e)); }',
          '__rhinoLocal.identityCustomAttrsOwnedKeys["managed/alpha_user/example"] = ["plain"];',
        ].join("\\n")),
      });
      // The script catches the refusal, but it is a harness fault (E19), so
      // the job fails naming it, and expectHarnessFault is reachable from the
      // installed package.
      expect(response.outcome, JSON.stringify(response)).toBe("runtime_error");
      expect(response.error?.message).toMatch(/^rhino-local harness fault identity-bag-key-collision: openidm.patch:.*collides with/);
    } finally { await runner.close(); }
  });

  // A script context's overlay is a separate packaged asset; it must load
  // from the installed package, and the case types must accept the context.
  it("runs an access-token modification case", async () => {
    const runner = await RhinoRunner.spawn();
    try {
      const run = await runCase(runner, defineCase({
        name: "token", context: "OAUTH2_ACCESS_TOKEN_MODIFICATION",
        script: 'accessToken.setField("owner", String(identity.getName()));',
        given: { engine: "legacy", accessToken: { clientId: "widget-client" } },
        expect: { accessToken: { fields: { added: { owner: "widget-client" } } } },
      }));
      expect(run.verdict.summary, run.verdict.summary).toBe("");
    } finally { await runner.close(); }
  });

  // The OIDC claims overlay is a second packaged asset, and its result is
  // the script's completion value, read through the runner's completion hook.
  it("runs an OIDC claims case on both engines", async () => {
    const runner = await RhinoRunner.spawn();
    try {
      for (const [engine, script] of [
        ["next-gen", '({ values: { owner: String(identity.getName()) } });'],
        ["legacy", 'var v = new java.util.HashMap(); v.put("owner", identity.getName()); new org.forgerock.oauth2.core.UserInfoClaims(v, null);'],
      ] as const) {
        const run = await runCase(runner, defineCase({
          name: \`claims \${engine}\`, context: "OIDC_CLAIMS", script,
          given: { engine, identity: { name: "widget-user" } },
          expect: { oidc: { userinfo: { owner: "widget-user" } } },
        }));
        expect(run.verdict.summary, run.verdict.summary).toBe("");
      }
    } finally { await runner.close(); }
  });

  // The social IdP overlay is a third packaged asset; its result is also the
  // script's completion value, and its node-state writes are judged.
  it("runs a social IdP profile transformation case on both engines", async () => {
    const runner = await RhinoRunner.spawn();
    try {
      for (const [engine, script] of [
        ["next-gen", 'nodeState.putShared("idp", selectedIdp); ({ id: rawProfile.sub, mail: rawProfile.get("email") });'],
        ["legacy", 'nodeState.putShared("idp", selectedIdp); ({ id: String(rawProfile.get("sub").getObject()), mail: String(rawProfile.get("email").getObject()) });'],
      ] as const) {
        const run = await runCase(runner, defineCase({
          name: \`social \${engine}\`, context: "SOCIAL_IDP_PROFILE_TRANSFORMATION", script,
          given: { engine, socialIdp: { selectedIdp: "widget-idp", rawProfile: { sub: "w-1", email: "w@example.com" } } },
          expect: { socialIdp: { profile: { id: "w-1", mail: "w@example.com" } }, sharedState: { added: { idp: "widget-idp" } } },
        }));
        expect(run.verdict.summary, run.verdict.summary).toBe("");
      }
    } finally { await runner.close(); }
  });

  // Positive control for step 1: were the declarations to resolve to \`any\`,
  // this directive would be unused and tsc would fail.
  it("types the inputs from the suite's schema", () => {
    const wrong = () =>
      // @ts-expect-error userId is required by the suite's inputs
      lease.run({}).expect({ outcome: "known" });
    expect(typeof wrong).toBe("function");
  });

  it("exports the case, provider, diagnostics and profile surfaces", () => {
    expect(typeof validateCase).toBe("function");
    expect(typeof parseHarvest).toBe("function");
    expect(typeof readFailures).toBe("function");
    expect(typeof runShowLog).toBe("function");
    expect(profilePath("sandbox")).toMatch(/[.]aic-script-tester[/]profiles[/]sandbox[.]json$/);
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
[[ -f "$installed/src/bindings/rhino/identity-policy.cjs" ]] || fail "the tarball ships no canonical identity policy"
[[ ! -e "$installed/test" ]] || fail "the tarball ships the test suite"

(cd "$consumer" && npx tsc -p .) || fail "the consumer does not type-check against the shipped declarations"

jre="$WORK/jre"
mkdir -p "$jre/bin"
ln -s "$java_bin" "$jre/bin/java"
cache="$WORK/cache"
(
  cd "$consumer"
  env -u JAVA_HOME AIC_SCRIPT_TESTER_JAVA_HOME="$jre" AIC_SCRIPT_TESTER_CACHE="$cache" \
    AIC_SCRIPT_TESTER_RHINO_JAR="$jar" CI=1 npx vitest run
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
state="$consumer/.aic-script-tester"
[[ -f "$state/failures/failures.jsonl" ]] || fail "no failure record in $state"
[[ "$(cat "$state/.gitignore")" == "*" ]] || fail "$state does not ignore itself"
[[ "$(stat -c %a "$state/failures/failures.jsonl")" == 600 ]] || fail "the failure record is not 0600"
[[ -z "$(git -C "$consumer" status --porcelain -- .aic-script-tester)" ]] || fail "git would commit $state"

# A bin is its own process, so a callback provider reaches it only through
# --provider-module. With one record written above, show-log must load the
# module and use its provider (whose log reader answers without a network).
cat >"$consumer/provider.mjs" <<'JS'
export default {
  describe: async () => ({ name: "tenant", baseUrl: "https://tenant.example.com" }),
  getToken: async () => "unused",
  logs: { transaction: async () => [{ from: "provider-module" }] },
};
JS
shown="$(cd "$consumer" && npx aic-script-tester-show-log --stdout --provider-module provider.mjs)" ||
  fail "aic-script-tester-show-log failed with --provider-module"
[[ "$shown" == *'"from": "provider-module"'* ]] || fail "show-log did not read through the provider module"
[[ ! -e "$installed/.aic-script-tester" ]] || fail "state was written into node_modules"

fetched="$(cd "$consumer" && AIC_SCRIPT_TESTER_CACHE="$cache" AIC_SCRIPT_TESTER_RHINO_JAR="$jar" npx aic-script-tester-fetch-jar)"
[[ "$fetched" == "$jar" ]] || fail "aic-script-tester-fetch-jar printed $fetched, expected $jar"

echo "rhino-local-pack-smoke: ok ($(basename "$tarball"))"
