import { describe, expect, it } from "vitest";
import { z } from "zod";
import { validateCase } from "../../src/case/validate.ts";
import { deepEqual } from "../../src/case/equal.ts";
import { matchesValue } from "../../src/case/matcher.ts";
import type { Case, ExpectedValue } from "../../src/case/types.ts";
import { judge } from "../../src/case/verdict.ts";
import { bucket, makeCase, makeEffects } from "./helpers.ts";

describe("JSON value matchers", () => {
  const uuid = "123e4567-e89b-42d3-a456-426614174000";

  it("rejects sparse expectations and cannot judge their holes as wildcards", () => {
    const sparse = new Array(1) as ExpectedValue[];
    const input = { name: "sparse", script: "1", expect: {
      outcome: "true", sharedState: { added: { items: sparse } },
    } };
    expect(() => validateCase(input)).toThrow(
      /case\.expect\.sharedState\.added\.items\[0\] is missing; sparse arrays are not allowed/);
    expect(() => validateCase({ ...input, expect: { outcome: "true",
      callbacks: new Array(1) } })).toThrow(/case\.expect\.callbacks\[0\].*sparse arrays/);
    expect(() => validateCase({ ...input, given: { sharedState: { items: sparse } },
      expect: { outcome: "true" } })).toThrow(/case\.given\.sharedState\.items\[0\].*sparse arrays/);

    const unchecked = { ...input, given: {} } as unknown as Case;
    const effects = makeEffects({ sharedState: bucket({}, { items: ["unexpected"] }) });
    expect(() => judge(unchecked, effects)).toThrow(/items\[0\].*sparse arrays/);
    expect(matchesValue(sparse, ["unexpected"])).toBe(false);
    expect(deepEqual(sparse, ["unexpected"])).toBe(false);
    expect(deepEqual(sparse, sparse)).toBe(false);
  });

  it("matches nested state shapes and keeps matched keys declared", () => {
    const kase = makeCase({ expect: { outcome: "true", sharedState: {
      added: { tracking: { id: z.string().uuid(), token: /^[0-9a-f]{32}$/, extra: z.unknown() } },
    } } });
    const effects = makeEffects({ sharedState: bucket({}, {
      tracking: { id: uuid, token: "a".repeat(32), extra: { random: 1 } },
    }) });
    expect(judge(kase, effects).pass).toBe(true);
    const wrong = makeEffects({ sharedState: bucket({}, {
      tracking: { id: "bad", token: "wrong", extra: 1 },
    }) });
    const verdict = judge(kase, wrong);
    expect(verdict.pass).toBe(false);
    expect(verdict.summary).toContain("schema (zod: string uuid)");
    expect(verdict.summary).toContain("/^[0-9a-f]{32}$/");
    expect(verdict.summary).not.toContain("[object Object]");
  });

  it("matches callback fields and openidm and HTTP bodies at any depth", () => {
    const kase = makeCase({ expect: {
      outcome: "true",
      callbacks: [{ type: "TextOutputCallback", message: /^tracking-[0-9]+$/ }],
      openidm: [{ method: "create", resource: "managed/alpha_user",
        body: { ids: [z.string().uuid()] } }],
      http: [{ url: "https://tenant.example.com/collect", method: "POST",
        body: { tracking: z.string().uuid() } }],
    } });
    const effects = makeEffects({
      callbacks: [{ type: "TextOutputCallback", message: "tracking-42" }],
      openidm: [{ method: "create", resource: "managed/alpha_user", body: { ids: [uuid] } }],
      http: [{ url: "https://tenant.example.com/collect", method: "POST", body: { tracking: uuid } }],
    });
    expect(judge(kase, effects).pass).toBe(true);
    expect(judge(kase, makeEffects({ ...effects,
      http: [{ url: "https://tenant.example.com/collect", method: "POST", body: { tracking: "bad" } }],
    })).pass).toBe(false);
  });

  it("requires matcher-declared bodies and callback fields to be present", () => {
    const kase = makeCase({ expect: {
      outcome: "true",
      callbacks: [{ type: "TextOutputCallback", message: z.unknown() }],
      openidm: [{ method: "create", resource: "managed/alpha_user", body: z.unknown() }],
      http: [{ url: "https://tenant.example.com/collect", body: z.unknown() }],
    } });
    const complete = makeEffects({
      callbacks: [{ type: "TextOutputCallback", message: null }],
      openidm: [{ method: "create", resource: "managed/alpha_user", body: null }],
      http: [{ url: "https://tenant.example.com/collect", method: "POST", body: null }],
    });
    expect(judge(kase, complete).pass).toBe(true);
    expect(judge(kase, makeEffects({ ...complete,
      callbacks: [{ type: "TextOutputCallback" }],
    })).pass).toBe(false);
    expect(judge(kase, makeEffects({ ...complete,
      openidm: [{ method: "create", resource: "managed/alpha_user" }],
    })).pass).toBe(false);
    expect(judge(kase, makeEffects({ ...complete,
      http: [{ url: "https://tenant.example.com/collect", method: "POST" }],
    })).pass).toBe(false);
  });

  it("requires nested matcher keys and array slots to be present", () => {
    const kase = makeCase({ expect: { outcome: "true", sharedState: { added: {
      tracking: { id: z.unknown() },
      ids: [z.unknown()],
    } } } });
    const missingKey = makeEffects({ sharedState: bucket({}, {
      tracking: {}, ids: [null],
    }) });
    expect(judge(kase, missingKey).pass).toBe(false);
    const missingSlot = makeEffects({ sharedState: bucket({}, {
      tracking: { id: null }, ids: new Array(1),
    }) });
    expect(() => judge(kase, missingSlot)).toThrow(/ids\[0\].*sparse arrays/);
  });

  it("rejects matchers in input seeds and HTTP stubs", () => {
    expect(() => validateCase({ name: "bad seed", script: "1", given: {
      sharedState: { tracking: /x/ },
    }, expect: { outcome: "true" } })).toThrow(/given\.sharedState\.tracking cannot contain a matcher/);
    expect(() => validateCase({ name: "bad stub", script: "1", given: { http: [{
      match: { url: "https://tenant.example.com" }, reply: { status: 200, body: z.string() },
    }] }, expect: { outcome: "true" } })).toThrow(/given\.http\[0\]\.reply\.body cannot contain a matcher/);
  });
});
