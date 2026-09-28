import { describe, expect, it } from "vitest";
import { loadBehaviour } from "./load-behaviour.ts";

function keysOf(value: object): string[] {
  const keys: string[] = [];
  for (const key in value) {
    if (Object.prototype.hasOwnProperty.call(value, key)) {
      keys.push(key);
    }
  }
  return keys;
}

describe("request maps", () => {
  it("exposes get/size/containsKey on headers, with case-insensitive lookup", () => {
    const sandbox = loadBehaviour({
      requestHeaders: {
        "X-Aic-Probe": ["alpha", "bravo"],
        "X-Aic-Probe-Single": ["solo"],
      },
    });
    const headers = sandbox.requestHeaders as {
      get: (key: string) => { length: number; size: () => number; get: (i: number) => string };
      containsKey: (key: string) => boolean;
      size: () => number;
      isEmpty: () => boolean;
      keySet: () => unknown;
    };
    const list = headers.get("x-aic-probe");
    expect(list).not.toBeNull();
    expect(list.size()).toBe(2);
    expect(list.length).toBe(2);
    expect(list.get(0)).toBe("alpha");
    expect(list.get(1)).toBe("bravo");
    expect(headers.get("X-Aic-Probe")?.get(0)).toBe("alpha");
    expect(headers.containsKey("X-AIC-PROBE")).toBe(true);
    expect(headers.containsKey("missing")).toBe(false);
    expect(headers.get("missing")).toBeNull();
    expect(headers.size()).toBe(2);
    expect(headers.isEmpty()).toBe(false);
    expect(() => headers.keySet()).toThrow(/class shutter/);
    expect(keysOf(headers).sort()).toEqual(["x-aic-probe", "x-aic-probe-single"]);
  });

  it("keeps requestParameters case-sensitive and disjoint from headers", () => {
    const sandbox = loadBehaviour({
      requestHeaders: { probeq: ["from-header"] },
      requestParameters: { probeq: ["alpha", "bravo"], realm: ["alpha"] },
    });
    const params = sandbox.requestParameters as {
      get: (key: string) => { includes: (v: string) => boolean; length: number } | null;
      containsKey: (key: string) => boolean;
    };
    const headers = sandbox.requestHeaders as {
      get: (key: string) => unknown;
    };
    expect(params.get("probeq")?.length).toBe(2);
    expect(params.get("realm")?.includes("alpha")).toBe(true);
    expect(params.get("Probeq")).toBeNull();
    expect(params.containsKey("probeq")).toBe(true);
    expect(headers.get("probeq")).toBeTruthy();
  });

  it("returns cookie values as strings, not lists", () => {
    const sandbox = loadBehaviour({
      requestCookies: { session: "abc" },
    });
    const cookies = sandbox.requestCookies as {
      get: (key: string) => string | null;
      containsKey: (key: string) => boolean;
      size: () => number;
    };
    expect(cookies.get("session")).toBe("abc");
    expect(cookies.get("missing")).toBeNull();
    expect(cookies.containsKey("session")).toBe(true);
    expect(cookies.size()).toBe(1);
  });
});
