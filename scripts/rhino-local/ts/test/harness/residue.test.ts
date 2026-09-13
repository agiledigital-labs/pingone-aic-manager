import { describe, expect, it } from "vitest";
import { describeResidue, findResidue } from "../../src/harness/residue.ts";

const alice = { _id: "alice", userName: "alice" };

describe("findResidue", () => {
  it("is clean when the store holds only what the ledger created", () => {
    expect(
      findResidue({ "managed/alpha_user": [alice] }, [
        { type: "managed/alpha_user", record: alice },
      ])
    ).toEqual([]);
  });

  it("reports a record the script created and cleanup missed", () => {
    const residue = findResidue(
      {
        "managed/alpha_user": [alice],
        "managed/idr_name_variants": [{ _id: "9c1f", userId: "alice" }],
      },
      [{ type: "managed/alpha_user", record: alice }]
    );
    expect(residue).toEqual([
      { resource: "managed/idr_name_variants/9c1f", reason: "created-by-script" },
    ]);
  });

  // The discriminating case for "diff the store, not the call log". A check
  // built on create/delete calls sees nothing here: no row was added and none
  // was left undeleted. The tenant is still dirty.
  it("reports a fixture the script mutated and left changed", () => {
    const residue = findResidue(
      { "managed/alpha_user": [{ _id: "alice", userName: "alice-renamed" }] },
      [{ type: "managed/alpha_user", record: alice }]
    );
    expect(residue).toEqual([
      { resource: "managed/alpha_user/alice", reason: "mutated-by-script" },
    ]);
  });

  it("ignores fields the fixture never declared", () => {
    const residue = findResidue(
      { "managed/alpha_user": [{ ...alice, _rev: "1", inetUserStatus: "active" }] },
      [{ type: "managed/alpha_user", record: alice }]
    );
    expect(residue).toEqual([]);
  });

  it("returns nothing when the lane could not observe a store", () => {
    expect(findResidue(undefined, [])).toEqual([]);
  });
});

describe("describeResidue", () => {
  it("is empty for a clean run, so callers can test the string", () => {
    expect(describeResidue([])).toBe("");
  });

  it("names each survivor and what to do", () => {
    const text = describeResidue([
      { resource: "managed/idr_name_variants/9c1f", reason: "created-by-script" },
    ]);
    expect(text).toContain("1 record survived");
    expect(text).toContain("managed/idr_name_variants/9c1f");
    expect(text).toContain("cleanup()");
  });
});
