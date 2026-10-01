import type { z } from "zod";
import type { Case, Expect, Given, JsonObject } from "../case/types.ts";
import type { Channels, RequestDraft, SuiteSpec, WireMap } from "./types.ts";
/** Shared-state key prefix the tenant's config library reads before the ESV. */
export declare const ESV_STATE_PREFIX = "esv.";
/**
 * Collapse the suite's `always` and one test's overrides into a single draft.
 *
 * Merging is per key, not per channel: a test that sets one header keeps the
 * suite's others. Replacing the whole channel would make `always` useless the
 * moment a test needed to add anything, which is the failure mode that drives
 * people to copy the defaults into every test and then let them drift.
 */
export declare function mergeChannels(always: Channels | undefined, override: Channels | undefined): RequestDraft;
/**
 * A header or parameter is `string | string[]`; the wire form is always a
 * list, because AM reports one list element per occurrence in send order
 * (verified 2026-09-08 on both engines). A bare string is therefore a
 * one-element list, not a separate case.
 */
export declare function normaliseWire(map: WireMap | undefined): Record<string, string[]>;
/**
 * Fold the parsed inputs and the ESV overrides into shared state.
 *
 * Inputs land under their own names and ESVs under `esv.<name>`, so a suite
 * declaring an input called `esv.x` would be ambiguous; that is rejected
 * rather than resolved by precedence, because either precedence is a
 * defensible guess and neither is visible at the call site.
 */
export declare function applyInputsAndEsv(draft: RequestDraft, input: Readonly<Record<string, unknown>>): void;
/** Parse a run's inputs against the suite's schema, or reject extras. */
export declare function parseInputs<TSchema extends z.ZodType>(spec: Pick<SuiteSpec<TSchema>, "name" | "inputs">, raw: unknown): Record<string, unknown>;
/**
 * Turn a resolved draft into a `Given`.
 *
 * `session` compiles to `given.existingSession`, whose shape was measured
 * 2026-09-14 on both evaluators: a String->String map, present only when the
 * request carries a session cookie. Values are coerced here rather than in the
 * mock, so the AIC lane — which has to put them through a mini journey's
 * `putSessionProperty` — sends exactly what the local lane seeded.
 */
export declare function toGiven(draft: RequestDraft, base?: Given, realm?: string): Given;
/** Assemble the `Case` both lanes are judged against. One definition. */
export declare function toCase<TSchema extends z.ZodType>(spec: Pick<SuiteSpec<TSchema>, "name" | "script" | "outcomes" | "libraries">, 
/**
 * Used verbatim. The vitest adapter supplies an already-qualified
 * "describe > test" path, so prefixing the suite name here would print it
 * twice in every failure message.
 */
caseName: string, draft: RequestDraft, expect: Expect, base?: Given): Case;
/**
 * The same `Case`, from a `Given` that is already resolved.
 *
 * A step chain's later passes cannot go through `toCase`: `toGiven` layers the
 * draft over the base, so the request's original seeds would win over what the
 * previous pass actually left in state — the chain would silently restart from
 * the top on every step.
 */
export declare function caseWithGiven<TSchema extends z.ZodType>(spec: Pick<SuiteSpec<TSchema>, "name" | "script" | "outcomes" | "libraries">, caseName: string, given: Given, expect: Expect): Case;
export type { JsonObject };
/**
 * Who the mini journey logs in as. The principal need not exist as a managed
 * object (measured 2026-09-14), so this is free; taking it from shared state
 * means a suite that already sets `username` gets a session for that user
 * without saying so twice.
 */
export declare function sessionPrincipal(draft: RequestDraft): string;
