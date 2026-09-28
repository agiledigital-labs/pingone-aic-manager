import { readFileSync } from "node:fs";
import { join } from "node:path";
import { defineCase } from "../../src/case/index.ts";
import type {
  CallbackEffect,
  Case,
  Expect,
  Given,
} from "../../src/case/types.ts";
import { repoRoot } from "../../src/paths.ts";

/** Load an AM LIBRARY fixture body for `require()` by script name. */
export function librarySource(file: string): string {
  return readFileSync(
    join(repoRoot, "scripts/rhino-script-tester/fixtures", file),
    "utf8"
  );
}

/**
 * The name `scripts/rhino-script-tester/update-script.sh` gives the tenant
 * script every probe runs as, so a runtime error's `(name#line)` suffix reads
 * the same locally as live.
 */
export const PROBE_SOURCE_NAME = "AIC Rhino Let Probe";

/**
 * Load a case's script from its origin fixture — the file the tenant runs —
 * rather than a copy, so the local run cannot drift from the live one and an
 * error's line number is the line AIC reports.
 */
export function loadOrigin(origin: string, rewrites: readonly Rewrite[] = []): string {
  let source = readFileSync(join(repoRoot, origin), "utf8");
  for (const [pattern, replacement] of rewrites) {
    const hits = source.match(new RegExp(pattern.source, `${pattern.flags.replace("g", "")}g`));
    if (hits?.length !== 1) {
      throw new Error(
        `rhino-local: rewrite ${String(pattern)} matched ${hits?.length ?? 0} times in ${origin}, not once — the fixture changed`
      );
    }
    source = source.replace(pattern, replacement);
  }
  return source;
}

/**
 * One substitution applied to an origin fixture before it runs, matched by
 * the fixture's **structure** (`var PROBE_USER = "…"`), never by the value it
 * replaces: identity probes name tenant users, and this is how a case points
 * them at placeholder records without repeating a tenant identifier here.
 * Each must match exactly once, so a fixture edit fails loudly.
 */
export type Rewrite = readonly [pattern: RegExp, replacement: string];

/**
 * A measured difference between the local harness and AIC, kept visible
 * rather than papered over. Every payload key **not** listed must still equal
 * the committed live payload. Each listed key's local value is pinned by a
 * snapshot, and where its live value is committed too (it is left out only
 * when it cannot be sanitised) the local value must still differ from it. So
 * closing the gap, or the local output drifting, fails and gets looked at.
 */
export interface KnownGap {
  /** What AIC does, what the harness does instead, and what would close it. */
  reason: string;
  /** Top-level payload keys that differ from the live run today. */
  differs: readonly string[];
}

/** HiddenValueCallback the probe fixtures emit on a successful live run. */
export function hiddenValue(payload: Record<string, unknown>): CallbackEffect[] {
  return [
    {
      type: "HiddenValueCallback",
      id: "result",
      value: JSON.stringify(payload),
    },
  ];
}

export interface BlockedBy {
  /** Binding method path, e.g. `callbacks.isEmpty`. */
  method: string;
  /**
   * Exact `Error.message` the JVM runner reported. Compared as a substring of
   * `runCase`'s thrown error so a later overlay that implements the method
   * fails this assertion and unblocks the case.
   */
  throw: string;
}

export interface RealEntry {
  kase: Case;
  /** Path of the original fixture, relative to the repo root. */
  origin: string;
  kind: "next-gen" | "legacy";
  /**
   * Present when the case cannot yet produce a verdict. The suite stays green
   * by skipping the pass assertion and instead asserting this throw still
   * happens.
   */
  blocked?: BlockedBy;
  /** AM library script bodies keyed by `require()` id. */
  libraries?: Record<string, string>;
  /** Present when local and live measurably differ; see {@link KnownGap}. */
  gap?: KnownGap;
}

export function realCase(input: {
  name: string;
  kind: "nextgen" | "legacy";
  origin: string;
  given?: Given;
  expect: Expect;
  blocked?: BlockedBy;
  libraries?: Record<string, string>;
  rewrites?: readonly Rewrite[];
  gap?: KnownGap;
}): RealEntry {
  const init: {
    name: string;
    script: string;
    given?: Given;
    expect: Expect;
  } = {
    name: input.name,
    script: loadOrigin(input.origin, input.rewrites),
    expect: input.expect,
  };
  let given: Given | undefined = input.given;
  if (input.kind === "legacy") {
    given = { ...(given ?? {}), engine: "legacy" };
  }
  if (given !== undefined) {
    init.given = given;
  }
  const entry: RealEntry = {
    kase: defineCase(init),
    origin: input.origin,
    kind: input.kind === "legacy" ? "legacy" : "next-gen",
  };
  if (input.blocked !== undefined) {
    entry.blocked = input.blocked;
  }
  if (input.libraries !== undefined) {
    entry.libraries = input.libraries;
  }
  if (input.gap !== undefined) {
    entry.gap = input.gap;
  }
  return entry;
}
