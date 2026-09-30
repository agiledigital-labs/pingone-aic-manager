import { describe, expect, it } from "vitest";
import { aicWhenEnabled, defineSuite, useLease } from "../../src/harness/index.ts";

const suite = defineSuite({
  name: "queued callbacks suspend",
  script: [
    'if (nodeState.get("stage") === null) {',
    '  nodeState.putShared("stage", "asked");',
    '  callbacksBuilder.nameCallback("Name");',
    '  action.goTo("done");',
    "} else {",
    '  nodeState.putShared("name", String(callbacks.getNameCallbacks().get(0)));',
    '  callbacksBuilder.textOutputCallback(0, "hello");',
    '  action.goTo("done");',
    "}",
  ].join("\n"),
  outcomes: ["done"],
});

const noCallbacks = defineSuite({
  name: "goTo without callbacks",
  script: 'action.goTo("done");',
  outcomes: ["done"],
});

describe("callback suspension", () => {
  const lease = useLease(suite, aicWhenEnabled("queued-callbacks-suspend"));
  const localLease = useLease(suite);
  const control = useLease(noCallbacks);

  it("discards goTo when callbacks are queued", async () => {
    // Regression: the recorder used to return "done" for this same pass.
    const run = await lease.run().expect({
      outcome: null,
      callbacks: [{ type: "NameCallback", prompt: "Name" }],
      sharedState: { added: { stage: "asked" } },
    });
    expect(run.effects.outcome).toBeNull();
    expect(run.effects.discardedOutcome).toBe("done");
    expect(run.verdict.summary).toBe("");
  });

  it("reports clearly when an outcome was expected but callbacks suspended", async () => {
    const run = await localLease.run().expect({
      outcome: "done",
      callbacks: [{ type: "NameCallback", prompt: "Name" }],
      sharedState: { added: { stage: "asked" } },
    });
    expect(run.verdict.summary).toMatch(/queued 1 callbacks, so AM suspends and discards the recorded outcome "done"/);
  });

  it("suspends again when the resumed pass queues callbacks", async () => {
    const run = await lease.run().step({
      expect: { callbacks: [{ type: "NameCallback", prompt: "Name" }],
        sharedState: { added: { stage: "asked" } } },
      reply: [{ type: "NameCallback", value: "alice" }],
    }).expect({
      outcome: null,
      callbacks: [{ type: "TextOutputCallback", messageType: "0", message: "hello" }],
      sharedState: { added: { name: "alice" } },
    });
    expect(run.effects.outcome).toBeNull();
    expect(run.effects.discardedOutcome).toBe("done");
  });

  it("keeps an outcome when no callbacks were queued", async () => {
    const run = await control.run().expect({ outcome: "done" });
    expect(run.effects.outcome).toBe("done");
    expect(run.effects.discardedOutcome).toBeNull();
  });
});
