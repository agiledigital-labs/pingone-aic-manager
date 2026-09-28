import { describe, expect, it } from "vitest";
import { MAX_PASSES, passId } from "../../src/aic/txid.ts";

const STEM = "11111111-2222-4333-8444-555555555555";

describe("passId", () => {
  it("ids for steps 1 and 10 in the same stem are not prefixes of one another", () => {
    const one = passId(STEM, 1);
    const ten = passId(STEM, 10);
    // AM's log query is a prefix match. Unpadded `-1` is a prefix of `-10`, so
    // step 1's fetch silently absorbs step 10. Padding is the whole point.
    expect(ten.startsWith(one)).toBe(false);
    expect(one.startsWith(ten)).toBe(false);
    expect(one).toBe(`${STEM}-01`);
    expect(ten).toBe(`${STEM}-10`);
  });

  it("a chain's per-pass ids all share the stem, and the stem is a prefix of each", () => {
    const ids = [1, 2, 9, 10, 99].map((step) => passId(STEM, step));
    for (const id of ids) {
      expect(id.startsWith(`${STEM}-`)).toBe(true);
      expect(id.startsWith(STEM)).toBe(true);
    }
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("more than 99 passes errors rather than producing an ambiguous id", () => {
    expect(() => passId(STEM, MAX_PASSES + 1)).toThrow(/exceeds 99/);
    expect(() => passId(STEM, 100)).toThrow(/prefix of another/);
    expect(passId(STEM, MAX_PASSES)).toBe(`${STEM}-99`);
  });

  it("refuses a zero or non-integer step", () => {
    expect(() => passId(STEM, 0)).toThrow(/>= 1/);
    expect(() => passId(STEM, 1.5)).toThrow(/integer/);
  });
});
