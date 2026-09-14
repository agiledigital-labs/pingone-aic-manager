import { afterEach, describe, expect, it } from "vitest";
import {
  appendAicPass,
  beginSingleTrace,
  beginTrace,
  clearAicTrace,
  peekAicTrace,
  takeAicTrace,
} from "../../src/aic/trace.ts";

afterEach(() => {
  clearAicTrace();
});

describe("AIC trace slot", () => {
  it("beginTrace numbers passes under one stem", () => {
    const tx = beginTrace("sandbox", "stem-a");
    expect(tx.next()).toBe("stem-a-01");
    expect(tx.next()).toBe("stem-a-02");
    expect(peekAicTrace()).toEqual({
      stem: "stem-a",
      passIds: ["stem-a-01", "stem-a-02"],
      tenantName: "sandbox",
    });
  });

  it("takeAicTrace clears the slot so the next test cannot inherit it", () => {
    beginTrace("sandbox", "stem-b").next();
    const taken = takeAicTrace();
    expect(taken?.stem).toBe("stem-b");
    expect(takeAicTrace()).toBeUndefined();
  });

  it("beginSingleTrace is one id, not a numbered pass", () => {
    const id = beginSingleTrace("sandbox", "mint-uuid");
    expect(id).toBe("mint-uuid");
    expect(peekAicTrace()).toEqual({
      stem: "mint-uuid",
      passIds: ["mint-uuid"],
      tenantName: "sandbox",
    });
  });

  it("a later beginTrace replaces the mint trace", () => {
    beginSingleTrace("sandbox", "mint-uuid");
    beginTrace("sandbox", "subject-stem");
    expect(peekAicTrace()?.stem).toBe("subject-stem");
    expect(peekAicTrace()?.passIds).toEqual([]);
  });

  it("appendAicPass without a trace is an error, not a silent drop", () => {
    expect(() => appendAicPass("x-01")).toThrow(/without an active AIC trace/);
  });
});
