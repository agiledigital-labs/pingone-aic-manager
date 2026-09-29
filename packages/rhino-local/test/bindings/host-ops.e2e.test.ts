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
 * `utils.base64.encode.__parent__.__parent__.hostOp` still reaches it (checked
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
});
