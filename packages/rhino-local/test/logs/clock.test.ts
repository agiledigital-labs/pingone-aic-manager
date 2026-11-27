import { describe, expect, it } from "vitest";
import { clockFromEnv, parseClock, realClock, virtualAt, virtualClock } from "../../src/logs/clock.ts";
import { parseRfc3339Ns } from "../../src/logs/time.ts";

const ns = parseRfc3339Ns;
const good = {
  real_origin: "2026-11-22T09:00:00Z",
  virtual_origin: "2026-11-17T00:00:00Z",
  rate: 15,
};

describe("clock arithmetic", () => {
  it("is virtual_origin + (real - real_origin) * rate, at a fixed fake real time", () => {
    const spec = parseClock(JSON.stringify(good));
    // 4 real minutes at rate 15 is one virtual hour.
    const fakeReal = ns("2026-11-22T09:04:00Z");
    expect(virtualClock(spec, () => fakeReal).nowNs()).toBe(ns("2026-11-17T01:00:00Z"));
    // And the same instant to the nanosecond: 1 real ns is 15 virtual ns.
    expect(virtualAt(spec, ns("2026-11-22T09:00:00.000000001Z"))).toBe(ns("2026-11-17T00:00:00.000000015Z"));
  });

  it("runs before the origin too, and truncates a fractional rate toward zero", () => {
    const spec = parseClock(JSON.stringify({ ...good, rate: 0.5 }));
    expect(virtualAt(spec, ns("2026-11-22T08:59:59Z"))).toBe(ns("2026-11-16T23:59:59.5Z"));
    expect(virtualAt(spec, ns("2026-11-22T09:00:00.000000003Z"))).toBe(ns("2026-11-17T00:00:00.000000001Z"));
    expect(virtualAt(spec, ns("2026-11-22T08:59:59.999999999Z"))).toBe(ns("2026-11-17T00:00:00Z")); // -0.5 ns truncates toward zero
  });

  it("stays exact over spans a double cannot hold in nanoseconds", () => {
    // 400 real days at rate 15: the elapsed ns exceeds 2^53 by a wide margin.
    const spec = parseClock(JSON.stringify(good));
    const real = ns("2026-11-22T09:00:00Z") + 400n * 86_400n * 1_000_000_000n + 1n;
    expect(virtualAt(spec, real)).toBe(ns("2026-11-17T00:00:00Z") + 400n * 86_400n * 15_000_000_000n + 15n);
  });

  it("is real time without KIKORI_CLOCK", () => {
    const fake = 1_700_000_000_123_456_789n;
    expect(clockFromEnv({}, () => fake).nowNs()).toBe(fake);
    expect(clockFromEnv({ KIKORI_CLOCK: "" }, () => fake).nowNs()).toBe(fake);
    expect(realClock(() => fake).nowNs()).toBe(fake);
  });

  it("reads the named file once, at start", () => {
    let reads = 0;
    const clock = clockFromEnv(
      { KIKORI_CLOCK: "/x/clock.json" },
      () => ns("2026-11-22T09:04:00Z"),
      () => {
        reads++;
        return JSON.stringify(good);
      },
    );
    clock.nowNs();
    clock.nowNs();
    expect(reads).toBe(1);
    expect(clock.nowNs()).toBe(ns("2026-11-17T01:00:00Z"));
  });
});

describe("clock file refusals", () => {
  for (const field of ["real_origin", "virtual_origin", "rate"] as const) {
    it(`refuses a file missing ${field}`, () => {
      const { [field]: _dropped, ...rest } = good;
      expect(() => parseClock(JSON.stringify(rest))).toThrow(new RegExp(`clock file: ${field}`));
    });
  }

  it.each([
    ["rate 0", `{"real_origin":"2026-11-22T09:00:00Z","virtual_origin":"2026-11-17T00:00:00Z","rate":0}`],
    ["negative rate", `{"real_origin":"2026-11-22T09:00:00Z","virtual_origin":"2026-11-17T00:00:00Z","rate":-1}`],
    ["rate 1e999 (Infinity once parsed)", `{"real_origin":"2026-11-22T09:00:00Z","virtual_origin":"2026-11-17T00:00:00Z","rate":1e999}`],
    ["rate as a string NaN", `{"real_origin":"2026-11-22T09:00:00Z","virtual_origin":"2026-11-17T00:00:00Z","rate":"NaN"}`],
    ["rate as a string Infinity", `{"real_origin":"2026-11-22T09:00:00Z","virtual_origin":"2026-11-17T00:00:00Z","rate":"Infinity"}`],
    ["rate null", `{"real_origin":"2026-11-22T09:00:00Z","virtual_origin":"2026-11-17T00:00:00Z","rate":null}`],
  ])("refuses %s", (_name, text) => {
    expect(() => parseClock(text)).toThrow(/clock file: rate must be a positive number/);
  });

  it("refuses a malformed origin, malformed JSON and a non-object", () => {
    expect(() => parseClock(JSON.stringify({ ...good, real_origin: "yesterday" }))).toThrow(/clock file: real_origin: /);
    expect(() => parseClock(JSON.stringify({ ...good, virtual_origin: 5 }))).toThrow(/clock file: virtual_origin must be/);
    expect(() => parseClock("{")).toThrow(/^Error: clock file|clock file:/);
    expect(() => parseClock("[]")).toThrow(/clock file: must be a JSON object/);
  });

  it("names the variable and path when the file cannot be read", () => {
    expect(() =>
      clockFromEnv({ KIKORI_CLOCK: "/nope" }, () => 0n, () => {
        throw new Error("ENOENT");
      }),
    ).toThrow(/KIKORI_CLOCK=\/nope: ENOENT/);
  });
});
