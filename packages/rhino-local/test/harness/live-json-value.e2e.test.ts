import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "json-value-import",
  script: [
    "var imported = JavaImporter(org.forgerock.json.JsonValue);",
    "var object = imported.JsonValue.object();",
    'object.put("a", "b");',
    "var qualified = org.forgerock.json.JsonValue.json(org.forgerock.json.JsonValue.object());",
    'action.goTo(typeof object === "object" && typeof object.put === "function" && typeof object.get === "function" && object.get("a") === "b" && String(object) === \'{ "a": "b" }\' && String(qualified) === "{  }" ? "match" : "mismatch");',
  ].join("\n"),
  outcomes: ["match", "mismatch"],
});

describe("JsonValue import", () => {
  const lease = useLease(suite, aicWhenEnabled("live-json-value"));

  it("supports the measured object and json calls", async () => {
    const run = await lease.run().expect({ outcome: "match" });
    expect(run.verdict.pass).toBe(true);
  });
});
