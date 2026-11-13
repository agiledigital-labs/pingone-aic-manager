/**
 * E19 on the real JVM: a harness fault reaches the runner's recorder from
 * every point it can be raised, and a script cannot take that channel away.
 */
import { afterAll, beforeAll, expect, it } from "vitest";
import { CaseRunError, mockPreamble, runCase, withHarvest } from "../../src/bindings/index.ts";
import { defineCase } from "../../src/case/index.ts";
import type { Given } from "../../src/case/types.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { RhinoRunner } from "../../src/runner.ts";

let runner: RhinoRunner;
beforeAll(async () => {
  runner = await RhinoRunner.spawn();
}, 60_000);
afterAll(async () => {
  await runner?.close();
}, 30_000);

const run = (script: string, given: Given = {}) =>
  runCase(
    runner,
    defineCase({
      name: "fault-channel",
      outcomes: ["true"],
      given,
      script: `${script}\naction.goTo("true");`,
      expect: { outcome: "true" },
    }),
    { timeoutMs: 5_000 },
  );

async function failure(promise: Promise<unknown>): Promise<CaseRunError> {
  const error = await promise.then(
    () => undefined,
    (e: unknown) => e,
  );
  expect(error).toBeInstanceOf(CaseRunError);
  return error as CaseRunError;
}

// Raised inside __rhinoLocalSeed, before the script: the recorder must already
// be captured, or the run is a plain runtime_error with no id and no tally row.
// Driven through the raw runner, as a host that skips case validation would
// (validateCase refuses an unknown binding key before the preamble is built).
it.each([
  ["seed-binding", { bindings: { fooBinding: {} } }, "given.bindings.fooBinding seeding is not implemented"],
  ["seed-cache-manager", { bindings: { cacheManager: { a: 1 } } }, "seeding caches is not implemented"],
])("a seed-time refusal is the harness fault %s", async (id, given, text) => {
  expectHarnessFault(id);
  const response = await runner.eval({
    preamble: mockPreamble(given as never),
    source: withHarvest('nodeState.putShared("r", "ran");'),
  });
  expect(response.outcome).toBe("runtime_error");
  expect(response.error?.message).toContain(text);
  expect(response.emulation?.faults?.map((f) => f.id)).toEqual([id]);
});

// The script catches the refusal after trying to switch the recorder off; the
// run must fail all the same.
it.each([
  ["replacing a __rhinoLocal property", "__rhinoLocal.faultSink = function () {};"],
  ["rebinding the fault helper", "__rhinoLocalFault = function () { return new Error('x'); };"],
  ["rebinding the sink holder", "__rhinoLocalFaultSink = { capture: function () {}, record: function () { return true; } };"],
  ["deleting the sink holder", "delete __rhinoLocalFaultSink;"],
])("%s does not silence a fault", async (_name, tamper) => {
  expectHarnessFault("fixture/http");
  const error = await failure(
    run(`${tamper}\ntry { httpClient.send("https://x.example.com/").get(); } catch (e) {}`),
  );
  expect(error.response.emulation?.faults?.map((f) => f.id)).toEqual(["fixture/http"]);
});

it("rebinding the arity helpers does not turn a guessed-range fault into a catchable error", async () => {
  expectHarnessFault("arity-unknown/map.get");
  const error = await failure(
    run(
      "__rhinoLocalExpectArity = function () {};\n" +
        "__rhinoLocalArityRefusal = function () { return new Error('x'); };\n" +
        "try { requestHeaders.get(); } catch (e) {}",
    ),
  );
  expect(error.response.emulation?.faults?.map((f) => f.id)).toEqual(["arity-unknown/map.get"]);
});

it.each([
  [
    "existingSession.get()",
    { existingSession: { UserId: "u" } },
    "InternalError: Can't find method org.forgerock.openam.scripting.javascript.MapScriptWrapper.get().",
  ],
  [
    "utils.crypto.randomUUID(1)",
    {},
    "InternalError: Can't find method org.forgerock.openam.scripting.bindings.crypto.ScriptCryptoService.randomUUID(number).",
  ],
])("%s with the wrong count is AIC's measured, catchable refusal", async (call, given, text) => {
  const { effects } = await run(
    `var r; try { ${call}; r = "no-throw"; } catch (e) { r = String(e); } nodeState.putShared("r", r);`,
    given as Given,
  );
  expect(effects.sharedState.final).toEqual({ r: text });
});
