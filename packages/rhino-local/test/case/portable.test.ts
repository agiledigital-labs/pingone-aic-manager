import { describe, expect, it } from "vitest";
import { isPortable, judge } from "../../src/case/index.ts";
import { makeCase, makeEffects } from "./helpers.ts";

describe("portability", () => {
  it("reports a case with no environment-dependent inputs as portable", () => {
    const kase = makeCase({
      given: {
        realm: "alpha",
        sharedState: { username: "alice" },
      },
      expect: { outcome: "true" },
    });
    expect(isPortable(kase)).toBe(true);
    expect(judge(kase, makeEffects()).portable).toBe(true);
  });

  it("reports a case with managed fixtures as not portable", () => {
    // Discriminating: a checker that only looked at esv (the design-note
    // wording) would call this portable. managed is an environment input.
    const kase = makeCase({
      given: {
        managed: {
          alpha_user: [{ userName: "alice", mail: ["a@example.com"] }],
        },
      },
      expect: { outcome: "true" },
    });
    expect(isPortable(kase)).toBe(false);
    expect(judge(kase, makeEffects()).portable).toBe(false);
  });

  it("reports esv, secrets and http stubs as not portable", () => {
    expect(
      isPortable(
        makeCase({
          given: { esv: { "esv-feature-flag": "true" } },
          expect: { outcome: "true" },
        })
      )
    ).toBe(false);
    expect(
      isPortable(
        makeCase({
          given: { secrets: { "my-signing-key": "placeholder" } },
          expect: { outcome: "true" },
        })
      )
    ).toBe(false);
    expect(
      isPortable(
        makeCase({
          given: {
            http: [
              {
                match: { url: /\/verify$/ },
                reply: { status: 200, body: {} },
              },
            ],
          },
          expect: { outcome: "true" },
        })
      )
    ).toBe(false);
  });

  it("treats empty environment maps and arrays as undeclared", () => {
    const kase = makeCase({
      given: { esv: {}, secrets: {}, managed: {}, http: [] },
      expect: { outcome: "true" },
    });
    expect(isPortable(kase)).toBe(true);
  });

  it("does not treat engine or realm as environment-dependent inputs", () => {
    expect(
      isPortable(
        makeCase({
          given: { engine: "legacy", realm: "bravo" },
          expect: { outcome: "true" },
        })
      )
    ).toBe(true);
  });
});
