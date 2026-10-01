import { describe, it } from "vitest";
import { registeredMapChain } from "./object-attributes-chain.ts";

// Measured 2026-10-01: a removal persists across the callback round trip.
describe("registered map removed on pass 1", () => {
  it("is null on pass 2", registeredMapChain("object-attributes-removed", 'nodeState.remove("objectAttributes");', { type: "null", value: "null" }));
});
