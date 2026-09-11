import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { defineCase } from "../../src/case/index.ts";
import type {
  CallbackEffect,
  Case,
  Expect,
  Given,
} from "../../src/case/types.ts";

const here = dirname(fileURLToPath(import.meta.url));

/** Directory holding next-gen / legacy corpus scripts. */
export const realCasesDir = here;

/**
 * Load a copied probe script. AM-lint-clean copies are `.cjs`; copies that
 * intentionally use a Rhino-banned construct (or `for each`) are `.src` so
 * `eslint.am.config.js` does not reject them.
 */
export function loadScript(kind: "nextgen" | "legacy", name: string): string {
  const base = join(here, kind, name);
  const cjs = `${base}.cjs`;
  const src = `${base}.src`;
  if (existsSync(cjs)) {
    return readFileSync(cjs, "utf8");
  }
  if (existsSync(src)) {
    return readFileSync(src, "utf8");
  }
  throw new Error(`rhino-local: missing real case script ${kind}/${name}`);
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
}

export function realCase(input: {
  name: string;
  kind: "nextgen" | "legacy";
  origin: string;
  given?: Given;
  expect: Expect;
  blocked?: BlockedBy;
}): RealEntry {
  const init: {
    name: string;
    script: string;
    given?: Given;
    expect: Expect;
  } = {
    name: input.name,
    script: loadScript(input.kind, input.name),
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
  return entry;
}
