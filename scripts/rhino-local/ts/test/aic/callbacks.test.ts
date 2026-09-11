import { describe, expect, it } from "vitest";
import { parseAuthenticateCallbacks } from "../../src/aic/callbacks.ts";
import { HARNESS_CALLBACK_ID } from "../../src/aic/constants.ts";

describe("parseAuthenticateCallbacks", () => {
  it("reads type straight off the HTTP field and flattens output", () => {
    const parsed = parseAuthenticateCallbacks({
      authId: "ignored",
      callbacks: [
        {
          type: "NameCallback",
          output: [
            { name: "prompt", value: "User Name" },
            { name: "_id", value: 0 },
          ],
          input: [{ name: "IDToken1", value: "" }],
        },
      ],
    });
    expect(parsed.dumpRaw).toBeUndefined();
    expect(parsed.callbacks).toEqual([{ type: "NameCallback", prompt: "User Name" }]);
  });

  it("strips the harness HiddenValueCallback and returns its payload", () => {
    const dump = JSON.stringify({ outcome: "true", final: { username: "alice" } });
    const parsed = parseAuthenticateCallbacks({
      callbacks: [
        {
          type: "HiddenValueCallback",
          output: [
            { name: "id", value: HARNESS_CALLBACK_ID },
            { name: "value", value: dump },
          ],
          input: [{ name: "IDToken1", value: HARNESS_CALLBACK_ID }],
        },
      ],
    });
    expect(parsed.dumpRaw).toBe(dump);
    expect(parsed.callbacks).toEqual([]);
  });

  it("keeps a subject's HiddenValueCallback", () => {
    const parsed = parseAuthenticateCallbacks({
      callbacks: [
        {
          type: "HiddenValueCallback",
          output: [
            { name: "id", value: "result" },
            { name: "value", value: "probe" },
          ],
        },
      ],
    });
    expect(parsed.dumpRaw).toBeUndefined();
    expect(parsed.callbacks).toEqual([
      { type: "HiddenValueCallback", id: "result", value: "probe" },
    ]);
  });
});
