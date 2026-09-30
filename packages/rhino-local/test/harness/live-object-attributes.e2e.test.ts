import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "registered-state-container",
  script: [
    'var attributes = nodeState.get("objectAttributes");',
    'attributes.put("probe", "value");',
    'action.goTo(attributes.get("probe") === "value" && nodeState.get("objectAttributes").get("probe") === "value" ? "match" : "mismatch");',
  ].join("\n"),
  outcomes: ["match", "mismatch"],
  always: { state: { shared: { objectAttributes: {} } } },
});

describe("registered state container", () => {
  const lease = useLease(suite, aicWhenEnabled("live-object-attributes"));

  it("persists a map write in shared state", async () => {
    const run = await lease.run().expect({
      outcome: "match",
      sharedState: { changed: { objectAttributes: { probe: "value" } } },
    });
    expect(run.verdict.pass).toBe(true);
  });
});
