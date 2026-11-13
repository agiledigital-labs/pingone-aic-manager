import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/index.ts";
import { defineCase } from "../../src/case/index.ts";
import { RhinoRunner } from "../../src/runner.ts";

/**
 * `java/HostOps.java` gives the utils mocks java.util.Base64, SecureRandom and
 * javax.crypto, which the AM allow-list withholds from scripts. The runner
 * defines it for the preamble only; if a script could reach it by name, the
 * local shutter would be more permissive than AIC's.
 *
 * Not a sandbox: Rhino's `__parent__` walks a mock's closure scopes, so
 * `utils.base64.encode.__parent__.method.__parent__.__parent__.hostOp` still reaches it (checked
 * 2026-09-29). Disabling that would also disable `__proto__`, which scripts use.
 * A script that walks mock internals is outside what the harness models.
 */
describe("the utils host bridge on the real Rhino engine", () => {
  let runner: RhinoRunner;

  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);

  afterAll(async () => {
    await runner.close();
  }, 30_000);

  it("is gone before the script runs, while the mocks that captured it still work", async () => {
    const result = await runCase(
      runner,
      defineCase({
        name: "host-ops-hidden",
        script: [
          "var probe = {",
          "  global: typeof __rhinoLocalHostOp,",
          "  viaThis: typeof this.__rhinoLocalHostOp,",
          '  encoded: utils.base64.encode("abc"),',
          "};",
          "logger.info(JSON.stringify(probe));",
          'action.goTo("true");',
        ].join("\n"),
        outcomes: ["true"],
        given: {},
        expect: { outcome: "true" },
      }),
      { timeoutMs: 5_000 },
    );
    expect(result.verdict.summary).toBe("");
    const probe = JSON.parse(result.effects.logs[0]?.message ?? "{}") as Record<
      string,
      unknown
    >;
    expect(probe).toEqual({
      global: "undefined",
      viaThis: "undefined",
      encoded: "YWJj",
    });
  });

  // So what the dispatcher accepts is the limit instead: nothing the public
  // utils surface refuses. ECDSA is the control that the path reaches it: an
  // RSA-2048 keygen here once outran the 5 s script budget under parallel load,
  // a false red. The test's own timeout also stays above the script budget.
  it("reached through __parent__, refuses what utils refuses", { timeout: 30_000 }, async () => {
    const result = await runCase(
      runner,
      defineCase({
        name: "host-ops-parent",
        script: [
          "var op = utils.base64.encode.__parent__.method.__parent__.__parent__.hostOp;",
          "function attempt(f) {",
          "  try { return String(typeof f()); } catch (e) { return 'threw'; }",
          "}",
          "var probe = {",
          "  reached: typeof op,",
          '  ecdsa: attempt(function () { return op("generateKeyPair", "ECDSA").publicKey; }),',
          '  dsa: attempt(function () { return op("generateKeyPair", "DSA"); }),',
          '  sha224: attempt(function () { return op("pbkdf2", utils.types.stringToBytes("p"), utils.types.stringToBytes("s"), 1, 128, "SHA-224"); }),',
          "};",
          "logger.info(JSON.stringify(probe));",
          'action.goTo("true");',
        ].join("\n"),
        outcomes: ["true"],
        given: {},
        expect: { outcome: "true" },
      }),
      { timeoutMs: 5_000 },
    );
    expect(result.verdict.summary).toBe("");
    const probe = JSON.parse(result.effects.logs[0]?.message ?? "{}") as Record<
      string,
      unknown
    >;
    expect(probe).toEqual({ reached: "function", ecdsa: "object", dsa: "threw", sha224: "threw" });
  });
});

// Sol ctx r10 P2-2. nodeState.get uses the same signature helper. Next-gen
// only.
describe("a regexp is function in a decision-node overload signature", () => {
  let runner: RhinoRunner;

  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);

  afterAll(async () => {
    await runner.close();
  }, 30_000);

  it("nodeState.get(/x/, 1) names get(function,number)", async () => {
    const result = await runCase(runner, defineCase({
      name: "regexp nodeState.get",
      script: [
        "var hit = false;",
        "try { nodeState.get(/x/, 1); } catch (e) {",
        "  hit = String(e).indexOf(\"get(function,number)\") >= 0;",
        "}",
        "nodeState.putShared(\"hit\", hit);",
        "action.goTo(\"true\");",
      ].join("\n"),
      outcomes: ["true"],
      given: { engine: "next-gen" },
      expect: { outcome: "true", sharedState: { added: { hit: true } } },
    }));
    expect(result.verdict.pass, result.verdict.summary).toBe(true);
    expect(result.effects.sharedState.final.hit).toBe(true);
  }, 60_000);

  // Sol ctx r11 P2-2. The same helper names a decision-node overload.
  // A constructor is java.lang.Class; a method reference stays function.
  const nodeSignature = async (name: string, value: string, needle: string) => {
    const result = await runCase(runner, defineCase({
      name,
      script: [
        "var hit = false;",
        `try { nodeState.get(${value}, 1); } catch (e) {`,
        `  hit = String(e).indexOf(${JSON.stringify(needle)}) >= 0;`,
        "}",
        "nodeState.putShared(\"hit\", hit);",
        "action.goTo(\"true\");",
      ].join("\n"),
      outcomes: ["true"],
      given: { engine: "next-gen" },
      expect: { outcome: "true", sharedState: { added: { hit: true } } },
    }));
    expect(result.verdict.pass, result.verdict.summary).toBe(true);
    expect(result.effects.sharedState.final.hit).toBe(true);
  };

  it("r11 P2-2 nodeState.get(java.lang.String, 1) names get(java.lang.Class,number)", async () => {
    await nodeSignature("r11 string ctor", "java.lang.String", "get(java.lang.Class,number)");
  }, 60_000);

  it("r11 P2-2 nodeState.get(java.util.HashSet, 1) names get(java.lang.Class,number)", async () => {
    await nodeSignature("r11 hashset ctor", "java.util.HashSet", "get(java.lang.Class,number)");
  }, 60_000);

  it("r11 P2-2 nodeState.get of a Java method reference stays get(function,number)", async () => {
    await nodeSignature("r11 method ref", "(new java.lang.String(\"s\")).isEmpty", "get(function,number)");
  }, 60_000);
});
