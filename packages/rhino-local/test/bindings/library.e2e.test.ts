/**
 * A direct LIBRARY run on the real JVM, compared with live captures.
 * test/fixtures/library/*.js ran as next-gen library scripts on a sandbox
 * tenant (2026-11-11), required from a decision node and from four OAuth
 * callers. The JSON files are what those callers recorded. Here the same
 * library bodies run through defineCase / runCase, and every observable the
 * direct runner claims must agree.
 *
 * The direct runner has no caller of its own. given.library.callerContext
 * only changes logger.getName()'s context segment. The class shutter stays
 * the decision node's, so new java.util.HashSet() answers here even when the
 * live OAuth callers refused it. That difference is asserted, not papered over.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { CaseRunError, runCase } from "../../src/bindings/run.ts";
import { defineCase } from "../../src/case/validate.ts";
import type { JsonObject, JsonValue, LibraryCallerContext } from "../../src/case/types.ts";
import { expectHarnessFault } from "../../src/harness/emulation-guard.ts";
import { packageRoot } from "../../src/paths.ts";
import { RhinoRunner } from "../../src/runner.ts";

const dir = join(packageRoot, "test", "fixtures", "library");
const LIB = readFileSync(join(dir, "lib.script.js"), "utf8");
const INNER = readFileSync(join(dir, "inner.script.js"), "utf8");
const REPLACE = readFileSync(join(dir, "replace.script.js"), "utf8");
const THROW_TOP = readFileSync(join(dir, "throwtop.script.js"), "utf8");

/** Placeholder. The captures store `<uuid>` where the caller's script id was. */
const SCRIPT_ID = "00000000-0000-4000-8000-0000000000c7";
/** Fifteen characters, and not the default cookie, so cookieKind stays other-len-15. */
const COOKIE = "zzzzzzzzzzzzzzz";

interface Snap {
  hashSet: string;
  loggerName: string;
  scriptNameV: string;
  [key: string]: string;
}

interface Surface {
  module: Snap;
  fn: Snap;
  nested: string;
  nestedName: string;
  nestedLogger: string;
  nestedNode: string;
  thisType: string;
  thisIsExports: boolean;
  thisIsModuleExports: boolean;
}

interface LiveReport {
  caller: string;
  surface?: Surface;
  echo?: JsonValue;
  multi?: JsonObject;
  nullEcho?: string;
  undef?: string;
  missing?: string;
  replaced?: number;
  boomError?: { t: string; name: string; msg: string };
  boomString?: { t: string; name: string; msg: string };
  moduleThrow?: { t: string; name: string; msg: string };
  required?: string;
  requireMsg?: string;
}

const readLive = (file: string): LiveReport =>
  JSON.parse(readFileSync(join(dir, file), "utf8")) as LiveReport;

const CALLERS: readonly {
  file: string;
  callerContext: LibraryCallerContext;
  scriptName: string;
}[] = [
  { file: "decision-ng.json", callerContext: "AUTHENTICATION_TREE_DECISION_NODE", scriptName: "zzemuC7-dn" },
  { file: "token-ng.json", callerContext: "OAUTH2_ACCESS_TOKEN_MODIFICATION_NEXT_GEN", scriptName: "zzemuC7-atm" },
  { file: "validate-ng.json", callerContext: "OAUTH2_VALIDATE_SCOPE_NEXT_GEN", scriptName: "zzemuC7-vs" },
  { file: "mayact-ng.json", callerContext: "OAUTH2_MAY_ACT_NEXT_GEN", scriptName: "zzemuC7-ma" },
  { file: "evaluate-ng.json", callerContext: "OAUTH2_EVALUATE_SCOPE_NEXT_GEN", scriptName: "zzemuC7-es" },
];

/**
 * The live OAuth callers refused HashSet. This runner uses the decision-node
 * shutter, so the emulator's answer is the decision-node capture's `ok-1`.
 * Rewrite that cell after checking the capture still says what was measured.
 */
function expectedSurface(live: Surface): JsonValue {
  const text = JSON.stringify(live).replaceAll("<uuid>", SCRIPT_ID);
  const surface = JSON.parse(text) as Surface;
  for (const snap of [surface.module, surface.fn]) {
    expect(snap.hashSet === "ok-1" || snap.hashSet === "err").toBe(true);
    snap.hashSet = "ok-1";
  }
  return surface as unknown as JsonValue;
}

describe("library context", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  const run = (
    name: string,
    script: string,
    library: { export: string; args?: JsonValue[]; callerContext?: LibraryCallerContext },
    extra: {
      scriptName?: string;
      libraries?: Record<string, string>;
      returns?: {
        status: "returned" | "thrown";
        value?: JsonValue;
        undefined?: true;
        phase?: "export" | "module";
        type?: string;
        name?: string;
        message?: string;
      };
      allowUndeclared?: { returns?: boolean };
    } = {},
  ) => runCase(runner, defineCase({
    name,
    context: "LIBRARY",
    script,
    given: {
      realm: "/alpha",
      scriptName: extra.scriptName ?? "zzemuC7-dn",
      loggerScriptId: SCRIPT_ID,
      cookieName: COOKIE,
      // The surface probe reads a missing alpha_user. An unseeded collection
      // is a harness fault even from inside the probe's own try.
      managed: { "managed/alpha_user": [] },
      ...(extra.libraries === undefined ? {} : { libraries: extra.libraries }),
      library,
    },
    expect: {
      ...(extra.allowUndeclared === undefined ? {} : { allowUndeclared: extra.allowUndeclared }),
      ...(extra.returns === undefined ? {} : { returns: extra.returns }),
    },
  }));

  it.each(CALLERS)("surface matches $file", async ({ file, callerContext, scriptName }) => {
    const live = readLive(file);
    expect(live.surface).toBeDefined();
    const { effects, verdict } = await run(
      `surface ${file}`,
      LIB,
      { export: "surface", callerContext },
      {
        scriptName,
        libraries: { "zzemuC7-inner": INNER },
        returns: { status: "returned", value: expectedSurface(live.surface!) },
      },
    );
    expect(verdict.summary, verdict.summary).toBe("");
    expect(effects.returns?.status).toBe("returned");
    expect(effects.outcome).toBeNull();
  }, 60_000);

  it("echo, multi and a null argument match the capture", async () => {
    const live = readLive("decision-ng.json");
    const echo = await run("echo", LIB, {
      export: "echo",
      args: [{ n: 1, s: "a", z: null, b: true, a: [1, "x"] }],
    }, { returns: { status: "returned", value: live.echo! } });
    expect(echo.verdict.summary, echo.verdict.summary).toBe("");

    const multi = await run("multi", LIB, { export: "multi", args: ["p", 2] }, {
      returns: { status: "returned", value: live.multi! },
    });
    expect(multi.verdict.summary, multi.verdict.summary).toBe("");

    expect(live.nullEcho).toBe("null");
    const nulled = await run("null", LIB, { export: "echo", args: [null] }, {
      returns: { status: "returned", value: null },
    });
    expect(nulled.verdict.summary, nulled.verdict.summary).toBe("");
  }, 60_000);

  it("a thrown Error and a thrown string match the capture", async () => {
    const live = readLive("decision-ng.json");
    const error = live.boomError!;
    const asError = await run("boom error", LIB, { export: "boom", args: ["error"] }, {
      returns: { status: "thrown", type: error.t, name: error.name, message: error.msg },
    });
    expect(asError.verdict.summary, asError.verdict.summary).toBe("");

    const text = live.boomString!;
    const asText = await run("boom string", LIB, { export: "boom", args: ["string"] }, {
      returns: { status: "thrown", type: text.t, name: text.name, message: text.msg },
    });
    expect(asText.verdict.summary, asText.verdict.summary).toBe("");
  }, 60_000);

  it("a throw while the library loads is phase module", async () => {
    const live = readLive("decision-ng.json");
    const error = live.moduleThrow!;
    const result = await runCase(runner, defineCase({
      name: "module throw",
      context: "LIBRARY",
      script: THROW_TOP,
      given: {
        library: { export: "unused" },
      },
      expect: {
        returns: { status: "thrown", phase: "module", type: error.t, name: error.name, message: error.msg },
      },
    }));
    expect(result.verdict.summary, result.verdict.summary).toBe("");
    expect(result.effects.returns?.phase).toBe("module");
  }, 60_000);

  it("module.exports replacement is what require returns", async () => {
    const live = readLive("decision-ng.json");
    expect(live.replaced).toBe(1);
    const result = await runCase(runner, defineCase({
      name: "replaced",
      context: "LIBRARY",
      script: REPLACE,
      given: { library: { export: "replaced" } },
      expect: { returns: { status: "returned", value: 1 } },
    }));
    expect(result.verdict.summary, result.verdict.summary).toBe("");
  }, 60_000);

  it("an export that returns undefined passes only when that is declared or allowed", async () => {
    const script = "exports.n = function () {};";
    const named = await run("undefined declared", script, { export: "n" }, {
      returns: { status: "returned", undefined: true },
    });
    expect(named.verdict.pass).toBe(true);

    const omitted = await run("undefined omitted", script, { export: "n" });
    expect(omitted.verdict.pass).toBe(true);

    const valued = await run("value omitted", "exports.n = function () { return 1; };", { export: "n" });
    expect(valued.verdict.pass).toBe(false);
    expect(valued.verdict.summary).toMatch(/returns/);

    const allowed = await run(
      "value allowed",
      "exports.n = function () { return 1; };",
      { export: "n" },
      { allowUndeclared: { returns: true } },
    );
    expect(allowed.verdict.pass).toBe(true);
  }, 60_000);

  it("nodeState is not in library scope", async () => {
    const result = await run(
      "nodeState",
      "exports.n = function () { return nodeState; };",
      { export: "n" },
      {
        returns: {
          status: "thrown",
          type: "object",
          name: "ReferenceError",
          message: 'ReferenceError: "nodeState" is not defined.',
        },
      },
    );
    expect(result.verdict.summary, result.verdict.summary).toBe("");
  }, 60_000);

  it("emailService.send is a library-unmeasured fault", async () => {
    expectHarnessFault("library-unmeasured/emailService.send");
    const error = await run(
      "email",
      "exports.n = function () { emailService.send('a', 'b', 'c'); return 1; };",
      { export: "n" },
      { returns: { status: "returned", value: 1 } },
    ).then(() => null, (caught: unknown) => caught);
    expect(error).toBeInstanceOf(CaseRunError);
    const ids = ((error as CaseRunError).response.emulation?.faults ?? []).map((fault) => fault.id);
    expect(ids).toContain("library-unmeasured/emailService.send");
  }, 60_000);

  it("a value JSON cannot carry is a library-unmeasured fault", async () => {
    expectHarnessFault("library-unmeasured/return");
    const error = await run(
      "function return",
      "exports.n = function () { return function () { return 1; }; };",
      { export: "n" },
      { returns: { status: "returned", undefined: true } },
    ).then(() => null, (caught: unknown) => caught);
    expect(error).toBeInstanceOf(CaseRunError);
    const ids = ((error as CaseRunError).response.emulation?.faults ?? []).map((fault) => fault.id);
    expect(ids).toContain("library-unmeasured/return");
  }, 60_000);

  it("a decision-node case still runs and rejects library fields", async () => {
    expect(() => defineCase({
      name: "library on a decision",
      script: "action.goTo('true');",
      given: { library: { export: "n" } },
      expect: { outcome: "true" },
    })).toThrow(/case\.given\.library/);
    expect(() => defineCase({
      name: "returns on a decision",
      script: "action.goTo('true');",
      expect: { outcome: "true", returns: { status: "returned", value: 1 } },
    })).toThrow(/case\.expect\.returns/);

    const result = await runCase(runner, defineCase({
      name: "decision still runs",
      script: "action.goTo('true');",
      expect: { outcome: "true" },
    }));
    expect(result.verdict.summary, result.verdict.summary).toBe("");
    expect(result.effects.outcome).toBe("true");
    expect(result.effects.returns).toBeUndefined();
  }, 60_000);

  it("refuses a legacy engine, a missing export name, and the reserved library id", async () => {
    expect(() => defineCase({
      name: "legacy",
      context: "LIBRARY",
      script: "exports.n = function () { return 1; };",
      given: { engine: "legacy", library: { export: "n" } },
      expect: { returns: { status: "returned", value: 1 } },
    })).toThrow(/no legacy engine/);

    expect(() => defineCase({
      name: "no export",
      context: "LIBRARY",
      script: "exports.n = function () { return 1; };",
      given: { library: { export: "" } },
      expect: { returns: { status: "returned", value: 1 } },
    })).toThrow(/export must name/);

    expect(() => defineCase({
      name: "undefined arg",
      context: "LIBRARY",
      script: "exports.n = function (v) { return v; };",
      given: { library: { export: "n", args: [undefined as unknown as JsonValue] } },
      expect: { returns: { status: "returned", undefined: true } },
    })).toThrow(/not a JSON value/);

    await expect(runCase(runner, defineCase({
      name: "reserved",
      context: "LIBRARY",
      script: "exports.n = function () { return 1; };",
      given: {
        library: { export: "n" },
        libraries: { __rhinoLocalSubject: "exports.n = function () { return 2; };" },
      },
      expect: { returns: { status: "returned", value: 1 } },
    }))).rejects.toThrow(/__rhinoLocalSubject is reserved/);
  }, 60_000);
});

describe("library captures on engines with no require()", () => {
  it.each(["decision-legacy.json", "token-legacy.json"])("%s records require as unbound", (file) => {
    const live = readLive(file);
    expect(live.required).toBe("err");
    expect(live.requireMsg).toBe('ReferenceError: "require" is not defined.');
  });
});
