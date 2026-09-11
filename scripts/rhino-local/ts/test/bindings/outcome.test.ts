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
      withIdentifiedUser: (username: string) => unknown;
      withStage: (stage: string) => unknown;
    };
    expect(action.goTo("true")).toBe(action);
    expect(action.withErrorMessage("no")).toBe(action);
    expect(action.withHeader("X-A: b")).toBe(action);
    expect(action.suspend("wait")).toBe(action);
    expect(action.withIdentifiedUser("alice")).toBe(action);
    expect(action.withStage("collect")).toBe(action);
  });

  it("records suspend as a callback and does not set an outcome", () => {
    const effects = runScript('action.suspend("Check your email");');
    expect(effects.outcome).toBeNull();
    expect(effects.callbacks).toEqual([
      { type: "SuspendedTextOutputCallback", message: "Check your email" },
    ]);
  });

  it("invokes additionalLogic with a resume URI and substitutes {0}", () => {
    const effects = runScript(
      [
        "action.suspend('Click: [{0}]', function (resumeUri) {",
        '  nodeState.putShared("uri", resumeUri);',
        "});",
      ].join("\n")
    );
    expect(effects.callbacks[0]).toEqual({
      type: "SuspendedTextOutputCallback",
      message: "Click: [https://rhino-local.invalid/resume]",
    });
    expect(effects.sharedState.final).toEqual({
      uri: "https://rhino-local.invalid/resume",
    });
  });

  it("chains the remaining action methods without setting an outcome", () => {
    const effects = runScript(
      [
        'action.withIdentifiedAgent("agent-1")',
        '.withDescription("d")',
        '.withLockoutMessage("locked")',
        '.putSessionProperty("k", "v")',
        '.removeSessionProperty("k")',
        ".withMaxSessionTime(60)",
        ".withMaxIdleTime(30);",
      ].join("\n")
    );
    expect(effects.outcome).toBeNull();
  });

  it("chains goTo after withErrorMessage", () => {
    const effects = runScript(
      'action.withErrorMessage("denied").withHeader("X-A: 1").goTo("false");'
    );
    expect(effects.outcome).toBe("false");
  });
});
