import { describe, expect, it } from "vitest";
import { loadBehaviour, runScript } from "./load-behaviour.ts";

describe("nodeState", () => {
  it("reads shared, transient, then secure in that lookup order", () => {
    const sandbox = loadBehaviour({
      sharedState: { k: "shared" },
      transientState: { k: "transient" },
      secureState: { k: "secure" },
    });
    const nodeState = sandbox.nodeState as {
      get: (key: string) => unknown;
      isDefined: (key: string) => boolean;
    };
    expect(nodeState.get("k")).toBe("transient");
    expect(nodeState.isDefined("k")).toBe(true);
    expect(nodeState.isDefined("missing")).toBe(false);
    expect(nodeState.get("missing")).toBeNull();
  });

  it("lets secure shadow shared when transient is empty", () => {
    const sandbox = loadBehaviour({
      sharedState: { k: "shared" },
      secureState: { k: "secure" },
    });
    const nodeState = sandbox.nodeState as { get: (key: string) => unknown };
    expect(nodeState.get("k")).toBe("secure");
  });

  it("putShared and putTransient honour the bucket split", () => {
    const effects = runScript(
      [
        'nodeState.putShared("fromShared", 1);',
        'nodeState.putTransient("fromTransient", 2);',
      ].join("\n"),
      { sharedState: { existing: "yes" } }
    );
    expect(effects.sharedState.initial).toEqual({ existing: "yes" });
    expect(effects.sharedState.final).toEqual({ existing: "yes", fromShared: 1 });
    expect(effects.transientState.final).toEqual({ fromTransient: 2 });
    expect(effects.secureState.final).toEqual({});
  });

  it("returns nodeState from putShared / putTransient", () => {
    const sandbox = loadBehaviour();
    const nodeState = sandbox.nodeState as {
      putShared: (key: string, value: unknown) => unknown;
      putTransient: (key: string, value: unknown) => unknown;
    };
    expect(nodeState.putShared("a", "b")).toBe(sandbox.nodeState);
    expect(nodeState.putTransient("c", "d")).toBe(sandbox.nodeState);
  });

  it("throws on undefined values rather than dropping the key", () => {
    const sandbox = loadBehaviour();
    const nodeState = sandbox.nodeState as {
      putShared: (key: string, value: unknown) => unknown;
    };
    expect(() => nodeState.putShared("k", undefined)).toThrow(
      /nodeState\.putShared: value is undefined/
    );
  });

  it("remove drops the key from every bucket", () => {
    const effects = runScript('nodeState.remove("k");', {
      sharedState: { k: "shared", keep: 1 },
      transientState: { k: "transient" },
      secureState: { k: "secure" },
    });
    expect(effects.sharedState.final).toEqual({ keep: 1 });
    expect(effects.transientState.final).toEqual({});
    expect(effects.secureState.final).toEqual({});
  });

  it("keys returns distinct names across buckets, once", () => {
    const sandbox = loadBehaviour({
      sharedState: { a: 1, b: 2 },
      transientState: { b: 9, c: 3 },
      secureState: { d: 4 },
    });
    const nodeState = sandbox.nodeState as {
      keys: () => { size: () => number; contains: (k: string) => boolean };
    };
    const keys = nodeState.keys();
    expect(keys.size()).toBe(4);
    expect(keys.contains("a")).toBe(true);
    expect(keys.contains("b")).toBe(true);
    expect(keys.contains("c")).toBe(true);
    expect(keys.contains("d")).toBe(true);
  });

  it("getObject merges maps across buckets; get returns the first hit", () => {
    const sandbox = loadBehaviour({
      sharedState: { objectAttributes: { a: 1, b: 2 } },
      transientState: { objectAttributes: { b: 9, c: 3 } },
    });
    const nodeState = sandbox.nodeState as {
      get: (key: string) => unknown;
      getObject: (key: string) => unknown;
    };
    expect(nodeState.get("objectAttributes")).toEqual({ b: 9, c: 3 });
    expect(nodeState.getObject("objectAttributes")).toEqual({
      a: 1,
      b: 9,
      c: 3,
    });
  });

  it("mergeShared adds keys without replacing the whole bucket", () => {
    const effects = runScript(
      'nodeState.mergeShared({ b: 2, objectAttributes: { k: 1 } });',
      {
        sharedState: { a: 1, objectAttributes: { k: 0, m: 2 } },
        transientState: { t: 1 },
      }
    );
    expect(effects.sharedState.final).toEqual({
      a: 1,
      b: 2,
      objectAttributes: { k: 1, m: 2 },
    });
    expect(effects.transientState.final).toEqual({ t: 1 });
  });

  it("mergeTransient writes the transient bucket only", () => {
    const effects = runScript('nodeState.mergeTransient({ t: 2, extra: 3 });', {
      sharedState: { a: 1 },
      transientState: { t: 1 },
    });
    expect(effects.sharedState.final).toEqual({ a: 1 });
    expect(effects.transientState.final).toEqual({ t: 2, extra: 3 });
  });

  it("does not bind legacy maps on next-gen", () => {
    const sandbox = loadBehaviour();
    expect(sandbox.sharedState).toBeUndefined();
    expect(sandbox.transientState).toBeUndefined();
  });

  it("binds legacy sharedState / transientState as views of the same buckets", () => {
    const sandbox = loadBehaviour({
      engine: "legacy",
      sharedState: { a: 1 },
      transientState: { b: 2 },
    });
    const shared = sandbox.sharedState as {
      get: (key: string) => unknown;
      put: (key: string, value: unknown) => void;
    };
    const transient = sandbox.transientState as {
      get: (key: string) => unknown;
      put: (key: string, value: unknown) => void;
    };
    const nodeState = sandbox.nodeState as { get: (key: string) => unknown };
    expect(shared.get("a")).toBe(1);
    expect(shared.get("b")).toBeNull();
    expect(transient.get("b")).toBe(2);
    shared.put("c", 3);
    expect(nodeState.get("c")).toBe(3);
  });

  it("seeds secureState as a readable bucket with no writer", () => {
    const effects = runScript("nodeState.get('token');", {
      secureState: { token: "s3cret" },
    });
    expect(effects.secureState.initial).toEqual({ token: "s3cret" });
    expect(effects.secureState.final).toEqual({ token: "s3cret" });
    const sandbox = loadBehaviour({ secureState: { token: "s3cret" } });
    const nodeState = sandbox.nodeState as {
      get: (key: string) => unknown;
      isDefined: (key: string) => boolean;
    };
    expect(nodeState.get("token")).toBe("s3cret");
    expect(nodeState.isDefined("token")).toBe(true);
    expect(sandbox.nodeState).not.toHaveProperty("putSecure");
  });
});
