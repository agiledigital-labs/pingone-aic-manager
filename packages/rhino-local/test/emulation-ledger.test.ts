/**
 * The main-process ledger that worker audit deltas are merged into
 * (src/emulation.ts EmulationAuditLedger). Messages can arrive late or be
 * re-sent until acknowledged, so the rule under test is "newest revision of
 * each entry wins, and nothing is dropped because a later message omitted it".
 * The end-to-end equivalence with whole-audit snapshots is
 * test/harness/audit-delta.e2e.test.ts.
 */
import { describe, expect, it } from "vitest";
import { EmulationAuditLedger, type EmulationAudit } from "../src/emulation.ts";

type Failure = EmulationAudit["failures"][number];
const failure = (sequence: number, causes: string[]): Failure => ({ sequence, context: `job ${sequence}`, causes, kind: "protocol", observed: ["x"] });
const audit = (revision: number, failures: Failure[], outstanding: EmulationAudit["outstanding"] = []): EmulationAudit => ({ id: "w1", revision, failures, outstanding });

describe("EmulationAuditLedger", () => {
  it("keeps an entry a later delta omits", () => {
    const ledger = new EmulationAuditLedger();
    ledger.accept(audit(1, [failure(1, ["a"])]));
    ledger.accept(audit(2, [failure(2, ["b"])]));
    expect(ledger.audits()).toEqual([audit(2, [failure(1, ["a"]), failure(2, ["b"])])]);
  });

  it("a late, older copy of an entry does not undo a newer one (a declaration that cleared its causes)", () => {
    const ledger = new EmulationAuditLedger();
    ledger.accept(audit(3, [failure(1, [])], []));
    ledger.accept(audit(2, [failure(1, ["a"])], [{ context: "eval e" }]));
    expect(ledger.audits()).toEqual([audit(3, [failure(1, [])], [])]);
  });

  it("a re-sent entry at the same revision replaces, never duplicates", () => {
    const ledger = new EmulationAuditLedger();
    ledger.accept(audit(1, [failure(1, ["a"])]));
    ledger.accept(audit(1, [failure(1, ["a"])]));
    expect(ledger.audits()[0]?.failures).toHaveLength(1);
  });

  it("a whole snapshot and the deltas that make it up give the same audit", () => {
    const deltas = new EmulationAuditLedger();
    deltas.accept(audit(1, [failure(1, ["a"])], [{ context: "eval e" }]));
    deltas.accept(audit(2, [failure(2, ["b"]), failure(1, [])], []));
    const snapshot = new EmulationAuditLedger();
    snapshot.accept(audit(2, [failure(1, []), failure(2, ["b"])], []));
    expect(deltas.audits()).toEqual(snapshot.audits());
  });

  it("keeps workers apart", () => {
    const ledger = new EmulationAuditLedger();
    ledger.accept(audit(1, [failure(1, ["a"])]));
    ledger.accept({ ...audit(1, [failure(1, ["b"])]), id: "w2" });
    expect(ledger.audits().map((entry) => [entry.id, entry.failures[0]?.causes])).toEqual([["w1", ["a"]], ["w2", ["b"]]]);
  });
});
