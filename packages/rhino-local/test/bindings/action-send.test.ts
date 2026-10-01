import vm from "node:vm";
import { describe, expect, it } from "vitest";
import { loadBehaviour, runScript } from "./load-behaviour.ts";

const IMPORT =
  "var frJava = JavaImporter(org.forgerock.openam.auth.node.api.Action, com.sun.identity.authentication.callbacks.HiddenValueCallback);";

const sendScript = [
  IMPORT,
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
        "var frJava = JavaImporter(org.forgerock.openam.auth.node.api);",
        'action = frJava.Action.goTo("true").build();',
      ].join("\n"),
      { engine: "legacy", callbacks: [] }
    );
    expect(effects.outcome).toBe("true");
    expect(effects.callbacks).toEqual([]);
  });

  it("reaches each class through its package", () => {
    const effects = runScript(
      [
        "var frJava = JavaImporter(org.forgerock.openam.auth.node.api, com.sun.identity.authentication.callbacks);",
        'action = frJava.Action.send(new frJava.HiddenValueCallback("k", "v")).build();',
      ].join("\n"),
      { engine: "legacy", callbacks: [] }
    );
    expect(effects.callbacks).toEqual([
      { type: "HiddenValueCallback", id: "k", value: "v" },
    ]);
  });

  it("scopes each class to the importer that names it", () => {
    const sandbox = loadBehaviour({ engine: "legacy", callbacks: [] });
    const probe = (expr: string): unknown =>
      vm.runInContext(expr, sandbox as vm.Context);
    expect(probe("typeof JavaImporter(org.forgerock.openam.auth.node.api.Action).HiddenValueCallback")).toBe("undefined");
    expect(probe("typeof JavaImporter(com.sun.identity.authentication.callbacks.HiddenValueCallback).Action")).toBe("undefined");
  });
});

// Negative controls: neither legacy class leaks onto an importer that does
// not name it, on next-gen (measured live, live-java-importer-scope) or legacy.
// The Node path has no `java` package; the `JavaImporter(java.util)` control
// runs on the JVM in e2e.test.ts.
describe.each(["next-gen", "legacy"] as const)(
  "%s importer without the legacy classes",
  (engine) => {
    it.each([
      ["empty", "JavaImporter()"],
      ["JsonValue", "JavaImporter(org.forgerock.json.JsonValue)"],
    ])("%s importer has no Action or HiddenValueCallback", (_label, importer) => {
      const sandbox = loadBehaviour({ engine, callbacks: [] });
      const probe = (expr: string): unknown =>
        vm.runInContext(expr, sandbox as vm.Context);
      expect(probe(`typeof ${importer}.Action`)).toBe("undefined");
      expect(probe(`typeof ${importer}.HiddenValueCallback`)).toBe("undefined");
    });
  }
);

// Next-gen hides both even from an importer that names them (measured live,
// live-java-importer-scope), so the result path stays legacy-only.
describe("next-gen importer naming the legacy classes", () => {
  it.each([
    ["Action", "JavaImporter(org.forgerock.openam.auth.node.api.Action)"],
    ["Action", "JavaImporter(org.forgerock.openam.auth.node.api)"],
    ["HiddenValueCallback", "JavaImporter(com.sun.identity.authentication.callbacks.HiddenValueCallback)"],
    ["HiddenValueCallback", "JavaImporter(com.sun.identity.authentication.callbacks)"],
  ])("has no %s through %s", (member, importer) => {
    const sandbox = loadBehaviour({ callbacks: [] });
    expect(vm.runInContext(`typeof ${importer}.${member}`, sandbox as vm.Context)).toBe("undefined");
  });
});
