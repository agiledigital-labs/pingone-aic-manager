import { readFileSync } from "node:fs";
import type { Given } from "../case/types.ts";
import { bindingsRuntimePath, generatedJsPath } from "../paths.ts";

/**
 * Generated stubs + behaviour overlay + seed. Eval'd as the runner `preamble`
 * so author line numbers on `source` stay intact.
 */
export function mockPreamble(given: Given = {}): string {
  const generated = readFileSync(generatedJsPath, "utf8");
  const runtime = readFileSync(bindingsRuntimePath, "utf8");
  return `${generated}\n${runtime}\n__rhinoLocalSeed(${serializeGiven(given)});\n`;
}

/** Append a harvest call without shifting author line numbers. */
export function withHarvest(script: string): string {
  return `${script}\n;__rhinoLocalHarvest();\n`;
}

function serializeGiven(given: Given): string {
  return JSON.stringify(given, (_key, value: unknown) => {
    if (value instanceof RegExp) {
      return { __regex: value.source, __flags: value.flags };
    }
    return value;
  });
}
