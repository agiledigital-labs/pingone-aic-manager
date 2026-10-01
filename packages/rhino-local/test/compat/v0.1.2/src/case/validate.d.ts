import type { Case, CaseInit } from "./types.ts";
/** Typed entry point for case authors. */
export declare function defineCase(input: CaseInit): Case;
/**
 * Runtime validation. Dynamic cases (and typos TypeScript never sees) must
 * fail here rather than assert nothing.
 */
export declare function validateCase(input: unknown): Case;
