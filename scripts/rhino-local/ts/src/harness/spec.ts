import type { z } from "zod";
import type { Case, Expect, Given, JsonObject } from "../case/types.ts";
import {
  AM_OWNED_SESSION_SET,
  DEFAULT_SESSION_PRINCIPAL,
  sessionFromPrincipal,
} from "../case/session.ts";
import type {
  Channels,
  RequestDraft,
  SuiteSpec,
  WireMap,
  WireValue,
} from "./types.ts";

/** Shared-state key prefix the tenant's config library reads before the ESV. */
export const ESV_STATE_PREFIX = "esv.";

/**
 * Collapse the suite's `always` and one test's overrides into a single draft.
 *
 * Merging is per key, not per channel: a test that sets one header keeps the
 * suite's others. Replacing the whole channel would make `always` useless the
 * moment a test needed to add anything, which is the failure mode that drives
 * people to copy the defaults into every test and then let them drift.
 */
export function mergeChannels(
  always: Channels | undefined,
  override: Channels | undefined
): RequestDraft {
  return {
    state: {
      shared: { ...(always?.state?.shared ?? {}), ...(override?.state?.shared ?? {}) },
      transient: {
        ...(always?.state?.transient ?? {}),
        ...(override?.state?.transient ?? {}),
      },
    },
    esv: { ...(always?.esv ?? {}), ...(override?.esv ?? {}) },
    headers: { ...normaliseWire(always?.headers), ...normaliseWire(override?.headers) },
    params: { ...normaliseWire(always?.params), ...normaliseWire(override?.params) },
    session: { ...(always?.session ?? {}), ...(override?.session ?? {}) },
    // Declared-empty and not-declared are different requests: `session: {}`
    // asks for a logged-in session with no extra properties, which is a real
    // case and is invisible if you only look at the merged key count.
    sessionRequested:
      always?.session !== undefined || override?.session !== undefined,
  };
}

/**
 * A header or parameter is `string | string[]`; the wire form is always a
 * list, because AM reports one list element per occurrence in send order
 * (verified 2026-09-08 on both engines). A bare string is therefore a
 * one-element list, not a separate case.
 */
export function normaliseWire(map: WireMap | undefined): Record<string, string[]> {
  const out: Record<string, string[]> = {};
  for (const [key, value] of Object.entries(map ?? {})) {
    out[key] = wireValues(key, value);
  }
  return out;
}

function wireValues(key: string, value: WireValue): string[] {
  if (typeof value === "string") {
    return [value];
  }
  if (!Array.isArray(value) || value.some((entry) => typeof entry !== "string")) {
    throw new Error(
      `rhino-local: ${key} must be a string or an array of strings`
    );
  }
  if (value.length === 0) {
    throw new Error(
      `rhino-local: ${key} is an empty array — a key present with no values cannot be sent; omit the key instead`
    );
  }
  return [...value];
}

/**
 * Fold the parsed inputs and the ESV overrides into shared state.
 *
 * Inputs land under their own names and ESVs under `esv.<name>`, so a suite
 * declaring an input called `esv.x` would be ambiguous; that is rejected
 * rather than resolved by precedence, because either precedence is a
 * defensible guess and neither is visible at the call site.
 */
export function applyInputsAndEsv(
  draft: RequestDraft,
  input: Readonly<Record<string, unknown>>
): void {
  for (const [key, value] of Object.entries(input)) {
    if (key.startsWith(ESV_STATE_PREFIX)) {
      throw new Error(
        `rhino-local: input ${JSON.stringify(key)} collides with the ${JSON.stringify(ESV_STATE_PREFIX)} namespace reserved for ESV overrides; rename the input`
      );
    }
    draft.state.shared[key] = value as JsonObject[string];
  }
  for (const [name, value] of Object.entries(draft.esv)) {
    draft.state.shared[`${ESV_STATE_PREFIX}${name}`] = value;
  }
}

/** Parse a run's inputs against the suite's schema, or reject extras. */
export function parseInputs<TSchema extends z.ZodType>(
  spec: Pick<SuiteSpec<TSchema>, "name" | "inputs">,
  raw: unknown
): Record<string, unknown> {
  if (spec.inputs === undefined) {
    if (raw !== undefined && Object.keys(raw as object).length > 0) {
      throw new Error(
        `rhino-local: ${spec.name} passed inputs but declares none; add an \`inputs\` schema to the suite`
      );
    }
    return {};
  }
  const result = spec.inputs.safeParse(raw ?? {});
  if (!result.success) {
    const detail = result.error.issues
      .map((issue) => `${issue.path.join(".") || "(root)"}: ${issue.message}`)
      .join("; ");
    throw new Error(`rhino-local: ${spec.name} input invalid — ${detail}`);
  }
  return result.data as Record<string, unknown>;
}

/**
 * Turn a resolved draft into a `Given`.
 *
 * `session` compiles to `given.existingSession`, whose shape was measured
 * 2026-09-14 on both evaluators: a String->String map, present only when the
 * request carries a session cookie. Values are coerced here rather than in the
 * mock, so the AIC lane — which has to put them through a mini journey's
 * `putSessionProperty` — sends exactly what the local lane seeded.
 */
export function toGiven(
  draft: RequestDraft,
  base: Given = {},
  realm = "alpha"
): Given {
  const given: Given = { ...base };
  if (draft.sessionRequested) {
    given.existingSession = {
      ...sessionFromPrincipal(sessionPrincipal(draft), realm),
      ...(base.existingSession ?? {}),
      ...sessionStrings(draft.session),
    };
  }
  if (Object.keys(draft.state.shared).length > 0) {
    given.sharedState = { ...(base.sharedState ?? {}), ...draft.state.shared };
  }
  if (Object.keys(draft.state.transient).length > 0) {
    given.transientState = {
      ...(base.transientState ?? {}),
      ...draft.state.transient,
    };
  }
  if (Object.keys(draft.headers).length > 0) {
    given.requestHeaders = { ...(base.requestHeaders ?? {}), ...draft.headers };
  }
  if (Object.keys(draft.params).length > 0) {
    given.requestParameters = {
      ...(base.requestParameters ?? {}),
      ...draft.params,
    };
  }
  return given;
}

/** Assemble the `Case` both lanes are judged against. One definition. */
export function toCase<TSchema extends z.ZodType>(
  spec: Pick<SuiteSpec<TSchema>, "name" | "script" | "outcomes">,
  /**
   * Used verbatim. The vitest adapter supplies an already-qualified
   * "describe > test" path, so prefixing the suite name here would print it
   * twice in every failure message.
   */
  caseName: string,
  draft: RequestDraft,
  expect: Expect,
  base: Given = {}
): Case {
  return {
    name: caseName,
    script: spec.script,
    outcomes: spec.outcomes,
    given: toGiven(draft, base),
    expect,
  };
}

export type { JsonObject };

/**
 * AM session properties are strings — every value in the measured 23-key
 * session was one, `AuthLevel: "0"` included. A number or boolean is coerced
 * (it survives the round trip unambiguously); an object or array is refused,
 * because `putSessionProperty` would stringify it to `[object Object]` on the
 * AIC lane while a structured mock would keep it here. That divergence is the
 * false-pass shape this harness exists to prevent.
 */
function sessionStrings(session: JsonObject): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [key, value] of Object.entries(session)) {
    if (AM_OWNED_SESSION_SET.has(key)) {
      throw new Error(
        `rhino-local: session.${key} is set by AM, not by the caller — a mini journey that tries to override it fails the whole login with an unexplained 401. ${key === "UserId" || key === "Principals" || key === "UserToken" || key === "Principal" || key === "sun.am.UniversalIdentifier" ? "Set `state.username` instead; the session is minted for that principal" : "Remove it"}`
      );
    }
    if (value === null || typeof value === "object") {
      throw new Error(
        `rhino-local: session.${key} must be a string — AM stores session properties as strings, so a ${value === null ? "null" : "structured value"} cannot survive the round trip`
      );
    }
    out[key] = String(value);
  }
  return out;
}

/**
 * Who the mini journey logs in as. The principal need not exist as a managed
 * object (measured 2026-09-14), so this is free; taking it from shared state
 * means a suite that already sets `username` gets a session for that user
 * without saying so twice.
 */
export function sessionPrincipal(draft: RequestDraft): string {
  const username = draft.state.shared.username;
  return typeof username === "string" && username.length > 0
    ? username
    : DEFAULT_SESSION_PRINCIPAL;
}
