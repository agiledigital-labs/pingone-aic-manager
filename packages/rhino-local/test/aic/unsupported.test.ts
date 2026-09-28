import { describe, expect, it } from "vitest";
import { aicUnsupportedReason } from "../../src/aic/unsupported.ts";
import { caseWith } from "./helpers.ts";

describe("aicUnsupportedReason", () => {
  it("allows a portable sharedState + requestHeaders case", () => {
    expect(
      aicUnsupportedReason(
        caseWith({
          given: {
            sharedState: { username: "alice" },
            requestHeaders: { "x-forwarded-for": ["10.0.0.1"] },
          },
        })
      )
    ).toBeUndefined();
  });

  it("skips author-declared managed data rather than use tenant state", () => {
    const reason = aicUnsupportedReason(
      caseWith({
        given: { managed: { alpha_user: [{ userName: "alice" }] } },
      })
    );
    expect(reason).toMatch(/managed/);
    expect(reason).toMatch(/environment-dependent/);
  });

  it("skips http stubs the same way", () => {
    const reason = aicUnsupportedReason(
      caseWith({
        given: {
          http: [{ match: { url: "/x" }, reply: { status: 200 } }],
        },
      })
    );
    expect(reason).toMatch(/http/);
  });

  it("skips an empty-looking http array as portable (no declaration)", () => {
    expect(
      aicUnsupportedReason(caseWith({ given: { http: [] } }))
    ).toBeUndefined();
  });

  it("skips secureState because next-gen has no putSecure", () => {
    expect(
      aicUnsupportedReason(
        caseWith({ given: { secureState: { token: "t" } } })
      )
    ).toMatch(/putSecure/);
  });

  it("skips legacy, resumedFromSuspend=true, and extra binding seeds", () => {
    expect(
      aicUnsupportedReason(caseWith({ given: { engine: "legacy" } }))
    ).toMatch(/legacy/);
    expect(
      aicUnsupportedReason(caseWith({ given: { resumedFromSuspend: true } }))
    ).toMatch(/resumedFromSuspend/);
    expect(
      aicUnsupportedReason(caseWith({ given: { resumedFromSuspend: false } }))
    ).toBeUndefined();
    expect(
      aicUnsupportedReason(caseWith({ given: { scriptName: "x" } }))
    ).toMatch(/scriptName/);
    expect(
      aicUnsupportedReason(caseWith({ given: { bindings: { logger: {} } } }))
    ).toMatch(/bindings/);
  });
});
