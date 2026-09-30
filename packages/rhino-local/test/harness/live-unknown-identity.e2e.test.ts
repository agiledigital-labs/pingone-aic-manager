import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "unknown-identity-stub",
  script: [
    'var identity = idRepository.getIdentity("00000000-0000-4000-8000-000000000000");',
    "var errors = [];",
    'try { identity.getAttributeValues("mail"); } catch (e) { errors.push(String(e)); }',
    'try { identity.setAttribute("mail", ["a@example.com"]); } catch (e) { errors.push(String(e)); }',
    "try { identity.store(); } catch (e) { errors.push(String(e)); }",
    'action.goTo(identity && typeof identity.getAttribute === "undefined" && errors.length === 3 && errors[0].indexOf("amIdentity") !== -1 && errors[1].indexOf("amIdentity") !== -1 && errors[2].indexOf("amIdentity") !== -1 ? "match" : "mismatch");',
  ].join("\n"),
  outcomes: ["match", "mismatch"],
});

describe("unknown identity stub", () => {
  const lease = useLease(suite, aicWhenEnabled("live-unknown-identity"));

  it("returns a non-null identity with measured null-backed failures", async () => {
    const run = await lease.run().expect({ outcome: "match" });
    expect(run.verdict.pass).toBe(true);
  });
});
