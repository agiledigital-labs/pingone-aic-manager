import { describe, expect, it } from "vitest";
import { HARNESS_CALLBACK_ID } from "../../src/aic/constants.ts";
import { emitResultScript } from "../../src/aic/emit-result.ts";
import { lintAmScript } from "./helpers.ts";

describe("emitResultScript", () => {
  it("dumps nodeState and bakes the routed outcome; it does not judge", () => {
    const source = emitResultScript("true");
    expect(source).toContain(HARNESS_CALLBACK_ID);
    expect(source).toContain("hiddenValueCallback");
    expect(source).toContain('"true"');
    expect(source).toContain("nodeState.get");
    expect(source).not.toContain("allowUndeclared");
    expect(source).not.toContain("mismatch");
    expect(source).not.toMatch(/case\.expect/);
    expect(source).not.toMatch(/added|changed|removed/);
  });

  it("does not goTo, so AM returns the HiddenValueCallback", () => {
    const source = emitResultScript("false");
    expect(source).not.toContain("action.goTo");
    expect(source).not.toMatch(/outcome\s*=/);
    expect(source).toContain('"false"');
  });

  it("passes AM Rhino lint for a custom outcome", async () => {
    const source = emitResultScript("created");
    expect(await lintAmScript(source, "generated/aic-result.cjs")).toEqual([]);
  });
});
