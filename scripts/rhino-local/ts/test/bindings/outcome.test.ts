import { describe, expect, it } from "vitest";
import { loadBehaviour, runScript } from "./load-behaviour.ts";

describe("outcome", () => {
  it("records outcome = \"x\" and action.goTo(\"x\") identically", () => {
    const assigned = runScript('outcome = "true";');
    const goneTo = runScript('action.goTo("true");');
    expect(assigned.outcome).toBe("true");
    expect(goneTo.outcome).toBe("true");
    expect(assigned.outcome).toBe(goneTo.outcome);
  });

  it("records null when the script sets neither", () => {
    const effects = runScript("1 + 1;");
    expect(effects.outcome).toBeNull();
  });

  it("returns the action builder from goTo / withErrorMessage / withHeader", () => {
    const sandbox = loadBehaviour();
    const action = sandbox.action as {
      goTo: (name: string) => unknown;
      withErrorMessage: (message: string) => unknown;
      withHeader: (header: string) => unknown;
      suspend: (text: string) => unknown;
    };
    expect(action.goTo("true")).toBe(action);
    expect(action.withErrorMessage("no")).toBe(action);
    expect(action.withHeader("X-A: b")).toBe(action);
    expect(() => action.suspend("wait")).toThrow(/not mocked: action\.suspend/);
  });

  it("chains goTo after withErrorMessage", () => {
    const effects = runScript(
      'action.withErrorMessage("denied").withHeader("X-A: 1").goTo("false");'
    );
    expect(effects.outcome).toBe("false");
  });
});
