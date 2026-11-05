import { mkdtempSync, readFileSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type { EmulationAudit, EmulationOwner } from "../../src/emulation.ts";
import { FAULT_TALLY_ENV, faultTallyPath, formatFaultTally, tallyFaults, writeFaultTally } from "../../src/harness/fault-tally.ts";

type Failure = EmulationAudit["failures"][number];
const owner = (id: string, file: string): EmulationOwner => ({ id, name: id, kind: "test", file });
const fault = (id: string, message = `${id} message`) => ({ id, message });
let sequence = 0;
function entry(o: EmulationOwner | undefined, faults: Array<{ id: string; message: string }>, declared: string[] = []): Failure {
  const observed = faults.map((f) => `${f.id}: ${f.message}`);
  return {
    sequence: ++sequence, context: `job ${sequence}`, ...(o ? { owner: o, file: o.file } : {}),
    causes: observed.filter((cause) => !declared.some((id) => cause.startsWith(`${id}: `))),
    kind: "missing", observed, faults,
  };
}
const audit = (id: string, failures: Failure[]): EmulationAudit => ({ id, revision: 1, failures, outstanding: [] });

describe("tallyFaults", () => {
  it("counts a case once per id however often it hit it, and across audits (workers)", () => {
    const t1 = owner("t1", "/r/a.test.ts");
    const t2 = owner("t2", "/r/b.test.ts");
    const tally = tallyFaults([
      // The discriminating case: t1 hits the same id in two jobs and twice in one.
      audit("w1", [entry(t1, [fault("x"), fault("x")]), entry(t1, [fault("x")])]),
      audit("w2", [entry(t2, [fault("x"), fault("y")])]),
    ], "/r");
    expect(tally.faults).toEqual([
      { id: "x", cases: 2, failed: 2, files: ["a.test.ts", "b.test.ts"], message: "x message" },
      { id: "y", cases: 1, failed: 1, files: ["b.test.ts"], message: "y message" },
    ]);
    expect(tally.unidentified).toEqual([]);
  });

  it("counts a declared fault but not as a failure", () => {
    const tally = tallyFaults([audit("w", [
      entry(owner("t1", "/r/a.test.ts"), [fault("x")], ["x"]),
      entry(owner("t2", "/r/a.test.ts"), [fault("x")]),
    ])]);
    expect(tally.faults).toEqual([{ id: "x", cases: 2, failed: 1, files: ["/r/a.test.ts"], message: "x message" }]);
  });

  it("keeps a fault with an id that looks like another's prefix apart", () => {
    // "arity/a" declared must not mark "arity/a.b" as declared.
    const tally = tallyFaults([audit("w", [entry(owner("t", "/f"), [fault("arity/a"), fault("arity/a.b")], ["arity/a"])])]);
    expect(tally.faults.map((row) => [row.id, row.failed])).toEqual([["arity/a", 0], ["arity/a.b", 1]]);
  });

  it("lists missing behaviour without an id by its cause, and an unowned entry as one case per file", () => {
    const missing: Failure = { sequence: 1, context: "job", file: "/r/c.test.ts", causes: ["resource route: x"],
      kind: "missing", observed: ["resource route: x"] };
    const internal: Failure = { sequence: 2, context: "job", causes: ["boom"], kind: "internal", observed: ["boom"] };
    const tally = tallyFaults([audit("w", [missing, { ...missing, sequence: 3 }, internal])], "/r");
    expect(tally.faults).toEqual([]);
    expect(tally.unidentified).toEqual([{ id: "resource route: x", cases: 1, failed: 1, files: ["c.test.ts"], message: "resource route: x" }]);
  });

  it("does not count a job's echo of a host-call fault as unidentified, but keeps another case's", () => {
    // The IDM emulator raises an id'd fault inside a host call; the JVM's job response then
    // repeats its cause without the id. Same case: one fault. Control: the bare cause in a
    // case that never saw the id stays unidentified.
    const t1 = owner("t1", "/r/a.test.ts");
    const t2 = owner("t2", "/r/a.test.ts");
    const cause = "openidm-query-filter: openidm-query-filter message";
    const echo = (o: EmulationOwner): Failure => ({ sequence: ++sequence, context: "job", owner: o, file: o.file,
      causes: [cause], kind: "missing", observed: [cause] });
    const tally = tallyFaults([audit("w", [entry(t1, [fault("openidm-query-filter")]), echo(t1), echo(t2)])], "/r");
    expect(tally.faults.map((row) => [row.id, row.cases])).toEqual([["openidm-query-filter", 1]]);
    expect(tally.unidentified.map((row) => [row.id, row.cases])).toEqual([[cause, 1]]);
  });

  it("orders by cases, then id", () => {
    const tally = tallyFaults([audit("w", [
      entry(owner("t1", "/f"), [fault("b")]), entry(owner("t2", "/f"), [fault("c"), fault("a")]), entry(owner("t3", "/f"), [fault("c")]),
    ])]);
    expect(tally.faults.map((row) => row.id)).toEqual(["c", "a", "b"]);
  });
});

describe("the tally file", () => {
  let dir: string | undefined;
  afterEach(() => { if (dir) rmSync(dir, { recursive: true, force: true }); dir = undefined; });

  it(`defaults under the state directory and honours ${FAULT_TALLY_ENV}`, () => {
    dir = mkdtempSync(join(tmpdir(), "rhino-tally-"));
    const env = { AIC_SCRIPT_TESTER_STATE_DIR: join(dir, "state") };
    expect(faultTallyPath(env)).toBe(join(dir, "state", "harness-faults", "tally.json"));
    const path = writeFaultTally({ version: 1, faults: [], unidentified: [] }, env);
    expect(JSON.parse(readFileSync(path, "utf8"))).toEqual({ version: 1, faults: [], unidentified: [] });
    expect(statSync(path).mode & 0o777).toBe(0o600);

    const custom = join(dir, "deep", "out.json");
    expect(writeFaultTally({ version: 1, faults: [], unidentified: [] }, { ...env, [FAULT_TALLY_ENV]: custom })).toBe(custom);
    expect(readFileSync(custom, "utf8")).toContain('"version": 1');
  });

  it("summarises nothing for a clean run, and marks declared cases", () => {
    expect(formatFaultTally({ version: 1, faults: [], unidentified: [] }, "/t")).toBe("");
    const text = formatFaultTally({ version: 1, faults: [{ id: "x", cases: 3, failed: 1, files: ["a", "b"], message: "m" }],
      unidentified: [{ id: "route", cases: 1, failed: 1, files: [], message: "route" }] }, "/t");
    expect(text).toContain("     3 cases  x  (2 files, 2 declared)");
    expect(text).toContain("     1 case   route");
    expect(text).toContain("tally: /t");
  });
});
