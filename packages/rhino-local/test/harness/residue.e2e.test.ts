import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { runCase } from "../../src/bindings/index.ts";
import { defineCase } from "../../src/case/index.ts";
import { RhinoRunner } from "../../src/runner.ts";
import { findResidue } from "../../src/harness/residue.ts";

const SEED = { _id: "alice", userName: "alice" };
const LEDGER = [{ type: "managed/alpha_user", record: SEED }];

describe("residue against a real JVM run", () => {
  let runner: RhinoRunner;

  beforeAll(async () => {
    runner = await RhinoRunner.spawn();
  }, 60_000);

  afterAll(async () => {
    await runner.close();
  }, 30_000);

  it("catches a record the script created and nothing cleaned up", async () => {
    const result = await runCase(
      runner,
      defineCase({
        name: "creates-a-variant",
        script: [
          'openidm.create("managed/idr_name_variants", "v1", { userId: "alice" });',
          'action.goTo("true");',
        ].join("\n"),
        outcomes: ["true"],
        given: {
          managed: {
            "managed/alpha_user": [SEED],
            "managed/idr_name_variants": [],
          },
        },
        expect: { outcome: "true", allowUndeclared: { openidmWrites: true } },
      }),
      { timeoutMs: 5_000 }
    );

    // The store has to reach the harness at all — this is the half that no
    // pure unit test could establish, because it depends on the mock runtime
    // actually emitting it.
    expect(result.effects.managedStore).toBeDefined();

    expect(findResidue(result.effects.managedStore, LEDGER)).toEqual([
      { resource: "managed/idr_name_variants/v1", reason: "created-by-script" },
    ]);
  });

  it("is clean when the script deletes what it created", async () => {
    const result = await runCase(
      runner,
      defineCase({
        name: "cleans-up-after-itself",
        script: [
          'openidm.create("managed/idr_name_variants", "v1", { userId: "alice" });',
          'openidm.delete("managed/idr_name_variants/v1", null);',
          'action.goTo("true");',
        ].join("\n"),
        outcomes: ["true"],
        given: {
          managed: {
            "managed/alpha_user": [SEED],
            "managed/idr_name_variants": [],
          },
        },
        expect: { outcome: "true", allowUndeclared: { openidmWrites: true } },
      }),
      { timeoutMs: 5_000 }
    );
    expect(findResidue(result.effects.managedStore, LEDGER)).toEqual([]);
  });
});
