import { describe, expect, it } from "vitest";
import { formatRfc3339Ns, parseRfc3339Ns } from "../../src/logs/time.ts";

describe("RFC 3339 nanoseconds", () => {
  it("parses to the nanosecond, with offsets", () => {
    expect(parseRfc3339Ns("1970-01-01T00:00:00Z")).toBe(0n);
    expect(parseRfc3339Ns("2026-11-25T00:00:00.000001Z") - parseRfc3339Ns("2026-11-25T00:00:00Z")).toBe(1_000n);
    expect(parseRfc3339Ns("2026-11-25T10:00:00+10:00")).toBe(parseRfc3339Ns("2026-11-25T00:00:00Z"));
    expect(parseRfc3339Ns("2026-11-25T00:00:00.123456789Z") % 1_000_000_000n).toBe(123_456_789n);
    expect(parseRfc3339Ns("2024-02-29T12:00:00Z")).toBe(1_709_208_000_000_000_000n);
  });

  it("refuses what is not a real instant", () => {
    for (const bad of ["2026-02-29T00:00:00Z", "2026-13-01T00:00:00Z", "2026-11-25T24:00:00Z", "2026-11-25", "2026-11-25T00:00:00", "2026-11-25T00:00:60Z", "2026-11-25T00:00:00.1234567890Z"]) {
      expect(() => parseRfc3339Ns(bad), bad).toThrow();
    }
  });

  it("formats back losslessly", () => {
    for (const text of ["2026-11-25T00:00:00Z", "2026-11-25T00:00:00.000001Z", "2026-11-25T23:59:59.123456789Z"]) {
      expect(formatRfc3339Ns(parseRfc3339Ns(text))).toBe(text);
    }
  });
});
