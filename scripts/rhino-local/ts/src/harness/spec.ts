import type { z } from "zod";
import type { Case, Expect, Given, JsonObject } from "../case/types.ts";
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
 * `session` is refused rather than dropped. A populated `existingSession` has
 * never been observed on either lane, so anything the harness put there would
 * be an invention that both lanes agreed on — the one failure shape a
 * two-lane design cannot detect.
 */
export function toGiven(draft: RequestDraft, base: Given = {}): Given {
  if (Object.keys(draft.session).length > 0) {
    throw new Error(
      "rhino-local: channel `session` is declared but not implemented — a populated existingSession has never been measured, so seeding one would test an invented shape. Remove it, or measure the binding first"
    );
  }
  const given: Given = { ...base };
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
  testName: string,
  draft: RequestDraft,
  expect: Expect,
  base: Given = {}
): Case {
  return {
    name: `${spec.name} › ${testName}`,
    script: spec.script,
    outcomes: spec.outcomes,
    given: toGiven(draft, base),
    expect,
  };
}

export type { JsonObject };
