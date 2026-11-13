/**
 * A required library runs in its own scope (docs/api/12, "Library scope when
 * required", measured 2026-11-11): the caller's bindings and the caller's own
 * top-level `var`s are unbound inside it, and reading one is the engine's
 * ReferenceError. Driven through the public case API, so the rule under test
 * is the runner's, not a copy of it.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/index.ts";
import { mockPreamble } from "../../src/bindings/preamble.ts";
import { validateCase } from "../../src/case/index.ts";
import { RhinoRunner } from "../../src/runner.ts";

describe("library scope when required from a decision node", () => {
  let runner: RhinoRunner;
  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);
  afterAll(async () => {
    await runner.close();
  });

  const run = (script: string, libraries: Record<string, string>, added: Record<string, string>) =>
    runCase(
      runner,
      validateCase({
        name: "library scope",
        script,
        outcomes: ["done"],
        given: { libraries, sharedState: { user: "alice" }, managed: { "managed/alpha_user": [] } },
        expect: { outcome: "done", sharedState: { added } },
      })
    );

  it("leaves the caller's bindings unbound inside the library (the review's probe)", async () => {
    await expect(
      run(
        'var mine = 1; require("lib").go();',
        {
          lib: [
            "exports.go = function () {",
            '  nodeState.putShared("seen", String(nodeState.get("user")) + typeof mine);',
            '  action.goTo("done");',
            "};",
          ].join("\n"),
        },
        { seen: "alicenumber" }
      )
    ).rejects.toThrow(/ReferenceError: "nodeState" is not defined\./);
  });

  it("leaves a caller's local var unbound inside the library", async () => {
    await expect(
      run(
        'var mine = 1; nodeState.putShared("seen", String(require("lib").go())); action.goTo("done");',
        { lib: "exports.go = function () { return mine; };" },
        { seen: "1" }
      )
    ).rejects.toThrow(/ReferenceError: "mine" is not defined\./);
  });

  it("faults a library's top-level read of a caller binding too", async () => {
    await expect(
      run('require("lib"); action.goTo("done");', { lib: 'exports.user = nodeState.get("user");' }, {})
    ).rejects.toThrow(/ReferenceError: "nodeState" is not defined\./);
  });

  it("leaves context unbound inside the library", async () => {
    await expect(
      run('require("lib"); action.goTo("done");', { lib: "exports.c = typeof context === 'undefined' ? null : context.toString();" }, {})
    ).resolves.toBeDefined();
    await expect(
      run('require("lib"); action.goTo("done");', { lib: "exports.c = context;" }, {})
    ).rejects.toThrow(/ReferenceError: "context" is not defined\./);
  });

  it("passes when the caller hands the library nodeState as an argument (the control)", async () => {
    const result = await run(
      'var mine = 1; require("lib").go(nodeState, mine); action.goTo("done");',
      {
        lib: [
          "exports.go = function (state, n) {",
          '  state.putShared("seen", String(state.get("user")) + typeof n);',
          "};",
        ].join("\n"),
      },
      { seen: "alicenumber" }
    );
    expect(result.verdict.pass).toBe(true);
  });

  it("passes a library that uses only the bindings a required library sees", async () => {
    const result = await run(
      'nodeState.putShared("seen", require("lib").describe()); action.goTo("done");',
      {
        lib: [
          'var inner = require("inner");',
          "exports.describe = function () {",
          '  logger.info("library ran");',
          "  return [",
          "    typeof nodeState, typeof action, typeof mine,",
          '    utils.base64.encode("zz"), String(openidm.read("managed/alpha_user/zz-missing")),',
          "    typeof httpClient, typeof systemEnv, typeof secrets, inner.value",
          '  ].join("|");',
          "};",
        ].join("\n"),
        inner: 'exports.value = "inner";',
      },
      { seen: "undefined|undefined|undefined|eno=|null|object|object|object|inner" }
    );
    expect(result.verdict.pass).toBe(true);
  });

  it("gives a library a runner caller seeds directly the same scope", async () => {
    const response = await runner.eval({
      preamble: mockPreamble({}, { libraries: { lib: "exports.go = function () { return typeof nodeState + '|' + nodeState; };" } }),
      source: 'require("lib").go();',
    });
    expect(response.outcome).toBe("runtime_error");
    expect(JSON.stringify(response.error)).toMatch(/\\"nodeState\\" is not defined/);
  });
});
