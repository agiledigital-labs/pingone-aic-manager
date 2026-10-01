import { describe, it } from "vitest";
import { registeredMapChain } from "./object-attributes-chain.ts";

// Measured 2026-10-01: the scalar persists; the registered map does not return.
describe("registered map replaced by a scalar on pass 1", () => {
  it("is the scalar on pass 2", registeredMapChain("object-attributes-replaced", 'nodeState.putShared("objectAttributes", "scalar");', { type: "string", value: "scalar" }));
});
