import { describe, expect, it } from "vitest";
import { loadBehaviour, runScript } from "./load-behaviour.ts";

const sendScript = [
  "var frJava = JavaImporter();",
  "action = frJava.Action.send(",
  '  new frJava.HiddenValueCallback("result", "{\\"ok\\":true}")',
  ").build();",
].join("\n");

describe("legacy Action.send", () => {
  it("records HiddenValueCallback and sets outcome ok", () => {
    const effects = runScript(sendScript, {
      engine: "legacy",
      callbacks: [],
    });
    expect(effects.outcome).toBe("ok");
    expect(effects.callbacks).toEqual([
      { type: "HiddenValueCallback", id: "result", value: '{"ok":true}' },
    ]);
  });

  it("leaves next-gen-only bindings undefined", () => {
    const sandbox = loadBehaviour({ engine: "legacy", callbacks: [] });
    expect(typeof sandbox.callbacksBuilder).toBe("undefined");
    expect(typeof sandbox.action).toBe("undefined");
    expect(typeof sandbox.openidm).toBe("undefined");
    expect(typeof sandbox.utils).toBe("undefined");
    expect(typeof sandbox.requestCookies).toBe("undefined");
    expect(typeof sandbox.require).toBe("undefined");
    expect(typeof sandbox.JavaImporter).toBe("function");
    expect(typeof sandbox.sharedState).toBe("object");
  });

  it("Action.goTo sets the named outcome", () => {
    const effects = runScript(
      [
        "var frJava = JavaImporter();",
        'action = frJava.Action.goTo("true").build();',
      ].join("\n"),
      { engine: "legacy", callbacks: [] }
    );
    expect(effects.outcome).toBe("true");
    expect(effects.callbacks).toEqual([]);
  });
});
