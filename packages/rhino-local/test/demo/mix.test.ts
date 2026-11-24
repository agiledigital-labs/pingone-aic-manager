import { describe, expect, it } from "vitest";
import { MixError, parseMix, ratePerHour } from "../../src/demo/mix.ts";
import { BreakError, parseBreaks } from "../../src/demo/breaks.ts";
import { mixJson } from "./rig.ts";
import { parseRfc3339Ns } from "../../src/logs/time.ts";

const clone = (): any => structuredClone(mixJson());

describe("mix validation", () => {
  it("accepts the fixture mix", () => {
    const mix = parseMix(mixJson());
    expect(Object.keys(mix.journeys)).toEqual(["tree-a"]);
    expect(mix.requests.map((r) => r.name)).toEqual(["token", "read"]);
  });

  const cases: [string, (m: any) => void, RegExp][] = [
    ["a real-looking email domain", (m) => (m.journeys["tree-a"].users.domain = "corp.com"), /synthetic people only/],
    ["a subdomain of a non-reserved domain", (m) => (m.journeys["tree-a"].users.domain = "example.com.evil.io"), /synthetic people only/],
    ["an IPv6 network", (m) => (m.journeys["tree-a"].networks[0].cidr = "2001:db8::/32"), /not an IPv4 CIDR/],
    ["a prefix length of 33", (m) => (m.journeys["tree-a"].networks[0].cidr = "198.51.100.0/33"), /not an IPv4 CIDR/],
    ["all-zero network shares", (m) => (m.journeys["tree-a"].networks[0].share = 0), /shares must not all be zero/],
    ["an unknown field", (m) => (m.journeys["tree-a"].colour = "red"), /unknown field/],
    ["abandon above 1", (m) => (m.journeys["tree-a"].abandon = 1.5), /abandon: must be a number from 0 to 1/],
    ["abandon without steps", (m) => (m.journeys["tree-a"].steps = []), /never stops part way/],
    ["a daily curve of 23 numbers", (m) => (m.journeys["tree-a"].rate.daily = Array(23).fill(1)), /exactly 24 numbers/],
    ["byHourOfWeek with perHour", (m) => (m.journeys["tree-a"].rate = { perHour: 1, byHourOfWeek: Array(168).fill(1) }), /cannot be combined/],
    ["a negative rate", (m) => (m.journeys["tree-a"].rate.perHour = -1), /perHour: must be a number/],
    ["an answer outcome that is not one of three", (m) => (m.journeys["tree-a"].steps[0].answers[0].outcome = "maybe"), /outcome: must be/],
    ["a non-string fill value", (m) => (m.journeys["tree-a"].steps[0].answers[0].fill.NameCallback = 5), /fill.NameCallback: must be a string/],
    ["version 2", (m) => (m.version = 2), /version: must be 1/],
    ["a duplicate request name", (m) => (m.requests[1].name = "token"), /used twice/],
    ["a request stream that is not an access stream", (m) => (m.requests[0].stream = "am-core"), /am-access" or "idm-access/],
    ["a status of FAIL", (m) => (m.requests[0].status[0].status = "FAIL"), /SUCCESSFUL" or "FAILED/],
    ["a p95 below the median", (m) => (m.journeys["tree-a"].latencyMs = { medianMs: 100, p95Ms: 10 }), /p95Ms must not be below/],
    ["a path without a leading slash", (m) => (m.requests[0].path = "oauth2"), /must start with/],
    ["an HTTP rule with a broken regex", (m) => (m.journeys["tree-a"].http = [{ match: "(", responses: [{ share: 1, status: 200 }] }]), /regular expression/],
  ];
  it.each(cases)("refuses %s", (_name, change, message) => {
    const m = clone();
    change(m);
    expect(() => parseMix(m)).toThrow(MixError);
    expect(() => parseMix(m)).toThrow(message);
  });

  it("reports every problem at once, each with its path", () => {
    const m = clone();
    m.journeys["tree-a"].users.domain = "corp.com";
    m.requests[0].path = "x";
    try {
      parseMix(m);
      expect.unreachable();
    } catch (error) {
      expect((error as MixError).problems).toHaveLength(2);
    }
  });

  it("refuses text that is not JSON and a mix with no traffic", () => {
    expect(() => parseMix("{")).toThrow(/not JSON/);
    expect(() => parseMix({ version: 1 })).toThrow(/no traffic/);
  });
});

describe("rate curves", () => {
  it("indexes the week from Monday 00:00 UTC", () => {
    const weekly = [1, 1, 5, 1, 1, 1, 1]; // Wednesday
    const mix = parseMix({ ...clone(), requests: [], journeys: { x: { ...clone().journeys["tree-a"], rate: { perHour: 10, weekly } } } });
    const curve = mix.journeys["x"]!.rate;
    // 2026-11-18 is a Wednesday, 2026-11-17 a Tuesday.
    expect(ratePerHour(curve, parseRfc3339Ns("2026-11-18T10:00:00Z"))).toBe(50);
    expect(ratePerHour(curve, parseRfc3339Ns("2026-11-17T10:00:00Z"))).toBe(10);
  });
});

describe("break file", () => {
  const w = { from: "2026-11-18T01:00:00Z", to: "2026-11-18T02:00:00Z" };
  it("reads kikori's four kinds", () => {
    const b = parseBreaks(
      JSON.stringify([
        { ...w, kind: "endpoint", path: "/am/x", share: 0.5, slower: 3 },
        { ...w, kind: "node-fails", node: "n1", share: 0.2 },
        { ...w, kind: "template", stream: "am-core", text: "boom <*>", per_hour: 100 },
        { ...w, kind: "surge", network: "203.0.113.0/24", per_hour: 50 },
      ]),
    );
    expect(b.map((x) => x.kind)).toEqual(["endpoint", "node-fails", "template", "surge"]);
    expect(b[0]).toMatchObject({ slower: 3, index: 0 });
  });
  it("defaults slower to 1", () => {
    expect(parseBreaks(JSON.stringify([{ ...w, kind: "endpoint", path: "/", share: 1 }]))[0]).toMatchObject({ slower: 1 });
  });
  it.each([
    ["an unknown field", { ...w, kind: "node-fails", node: "n", share: 1, extra: 1 }, /unknown field/],
    ["an unknown kind", { ...w, kind: "meteor" }, /kind must be/],
    ["an empty window", { from: w.to, to: w.from, kind: "node-fails", node: "n", share: 1 }, /window is empty/],
    ["a share above 1", { ...w, kind: "endpoint", path: "/", share: 1.1 }, /share must be a number/],
    ["a network that is not IPv4 CIDR", { ...w, kind: "surge", network: "nope", per_hour: 1 }, /not IPv4 CIDR/],
    ["a negative rate", { ...w, kind: "template", stream: "am-core", text: "x", per_hour: -1 }, /per_hour/],
    ["a stream that is not a log stream", { ...w, kind: "template", stream: "nope", text: "x", per_hour: 1 }, /not a log stream/],
  ])("refuses %s", (_n, entry, message) => {
    expect(() => parseBreaks(JSON.stringify([entry]))).toThrow(BreakError);
    expect(() => parseBreaks(JSON.stringify([entry]))).toThrow(message);
  });
  it("refuses a file that is not an array", () => {
    expect(() => parseBreaks("{}")).toThrow(/array/);
  });
});
