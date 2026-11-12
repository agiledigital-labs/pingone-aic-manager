import vm from "node:vm";
import { describe, expect, it } from "vitest";
import { loadBehaviour } from "./load-behaviour.ts";

// Negative controls: neither result class (Action, HiddenValueCallback) leaks
// onto an importer that does not name it (measured live,
// live-java-importer-scope). The Node path has no `java` package.
describe("importer without the result classes", () => {
  it.each([
    ["empty", "JavaImporter()"],
    ["JsonValue", "JavaImporter(org.forgerock.json.JsonValue)"],
  ])("%s importer has no Action or HiddenValueCallback", (_label, importer) => {
    const sandbox = loadBehaviour({ callbacks: [] });
    const probe = (expr: string): unknown =>
      vm.runInContext(expr, sandbox as vm.Context);
    expect(probe(`typeof ${importer}.Action`)).toBe("undefined");
    expect(probe(`typeof ${importer}.HiddenValueCallback`)).toBe("undefined");
  });
});

// Both are hidden even from an importer that names them (measured live,
// live-java-importer-scope).
describe("importer naming the result classes", () => {
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
