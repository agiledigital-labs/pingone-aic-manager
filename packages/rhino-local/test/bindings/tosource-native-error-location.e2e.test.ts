/**
 * Known fidelity gap, pinned: a native error raised inside the emulator's
 * `Function.prototype.toSource` wrapper (e.g. a Java-object receiver) names the
 * preamble's location, where bare Rhino names the author's line. The wrapper is
 * script source; moving it into Java would be needed to keep the author
 * location. Kind and message prefix still agree. docs/rhino-local-gaps.md.
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { mockPreamble, withHarvest } from "../../src/bindings/preamble.ts";
import { parseHarvest } from "../../src/bindings/harvest.ts";
import { RhinoRunner } from "../../src/runner.ts";

const source = `
var out;
try { Function.prototype.toSource.call(new java.util.ArrayList()); out = "no error"; } catch (e) { out = String(e); }
nodeState.putShared("r", out);
`;

describe("toSource on a Java receiver: native error location", () => {
  let runner: RhinoRunner;
  beforeAll(async () => { runner = await RhinoRunner.spawn(); }, 60_000);
  afterAll(async () => { await runner?.close(); }, 30_000);

  it("keeps the native kind and message but reports the preamble location", async () => {
    const response = await runner.eval({ preamble: mockPreamble({}), source: withHarvest(source) });
    expect(response.outcome, JSON.stringify(response.error)).toBe("ok");
    const r = String(parseHarvest(response.value as string).sharedState.final.r);
    expect(r).toMatch(/^InternalError: Cannot find default value for object\. \(<preamble>#\d+\)$/);
  });
});
