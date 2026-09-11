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

  it("leaves unimplemented nodeState methods throwing", () => {
    const sandbox = loadBehaviour();
    const nodeState = sandbox.nodeState as { remove: (key: string) => void };
    expect(() => nodeState.remove("k")).toThrow(
      /rhino-local: not mocked: nodeState\.remove/
    );
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
