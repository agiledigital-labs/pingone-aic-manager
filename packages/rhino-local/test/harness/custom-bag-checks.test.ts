import { describe, expect, it } from "vitest";
import { defineSuite, useLease } from "../../src/harness/index.ts";
import type { IdmHandle } from "../../src/harness/types.ts";

const resource = "managed/alpha_user/example";
const record = { _id: "example" };
const suite = defineSuite({
  name: "nonobject-bag-checks",
  script: `if (callbacks.isEmpty()) {
    var identity = idRepository.getIdentity("example");
    identity.setAttribute("fr-idm-custom-attrs", nodeState.get("values"));
    identity.store();
    callbacksBuilder.nameCallback("Continue");
  } else { action.goTo("done"); }`,
  outcomes: ["done"],
  beforeRun: ({ fixtures }) => fixtures.create("managed/alpha_user", record),
});

// Both public handles previously returned an invented materialized success.
for (const stage of ["step", "final"] as const) {
  describe(`${stage} public check handles with AM bag provenance (round 3 #4)`, () => {
    const lease = useLease(suite);
    for (const text of ['"x"', "1", "[]", "true", "null"]) {
      it.each(["read", "query", "delete"] as const)(
        `${text}: %s refuses without modifying the record or bag`,
        async (method) => {
          const check = async (idm: IdmHandle) => {
            const result =
              method === "query"
                ? idm.query("managed/alpha_user", { _id: "example" })
                : idm[method](resource);
            await expect(result).rejects.toThrow(
              method === "delete" && text === '"x"'
                ? /REST DELETE fails with HTTP 500/
                : /external handle behavior is unmeasured/,
            );
            // Filtering out the affected record still yields the measured missing-row shape.
            expect(await idm.read("managed/alpha_user/missing")).toBeNull();
            expect(
              await idm.query("managed/alpha_user", { _id: "missing" }),
            ).toEqual([]);
          };
          let draft = lease
            .run()
            .state({ shared: { values: [text] } })
            .step({
              expect: {
                callbacks: [{ type: "NameCallback", prompt: "Continue" }],
                identityWrites: [
                  {
                    identity: "example",
                    attribute: "fr-idm-custom-attrs",
                    values: [text],
                  },
                ],
              },
              reply: [{ type: "NameCallback", value: "continue" }],
              ...(stage === "step" ? { check } : {}),
            });
          if (stage === "final") draft = draft.check(check);
          const result = await draft.expect({ outcome: "done" });
          expect(result.verdict.pass, result.verdict.summary).toBe(true);
          expect(result.effects.managedStore).toEqual({
            "managed/alpha_user": [record],
          });
          expect(result.effects.identityCustomAttrs).toEqual({
            [resource]: [text],
          });
        },
      );
    }

    it("object-bag check deletion keeps the handle store and provenance consistent", async () => {
      const check = async (idm: IdmHandle) => {
        expect(await idm.read(resource)).toEqual(record);
        expect(
          await idm.query("managed/alpha_user", { _id: "example" }),
        ).toEqual([record]);
        await idm.delete(resource);
        expect(await idm.read(resource)).toBeNull();
      };
      let draft = lease
        .run()
        .state({ shared: { values: ["{}"] } })
        .step({
          expect: {
            callbacks: [{ type: "NameCallback", prompt: "Continue" }],
            identityWrites: [
              {
                identity: "example",
                attribute: "fr-idm-custom-attrs",
                values: ["{}"],
              },
            ],
          },
          reply: [{ type: "NameCallback", value: "continue" }],
          ...(stage === "step" ? { check } : {}),
        });
      if (stage === "final") draft = draft.check(check);
      const result = await draft.expect({ outcome: "done" });
      expect(result.effects.managedStore).toEqual({
        "managed/alpha_user": stage === "step" ? [] : [record],
      });
      // Final checks run on clones, so housekeeping cannot rewrite recorded evidence.
      expect(result.effects.identityCustomAttrs).toEqual(
        stage === "step" ? {} : { [resource]: ["{}"] },
      );
    });
  });
}
