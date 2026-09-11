import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { emitDts } from "./emit-dts.ts";
import { emitJs } from "./emit-js.ts";
import { loadContext } from "./load.ts";
import { bindingsJsonPath, generatedDir } from "./paths.ts";
import { classifyBinding, membersOf } from "./schema.ts";
import type { ContextsDocument, Element } from "./schema.ts";

export const generatedJsName = "scripted-decision-mocks.cjs";
export const generatedDtsName = "scripted-decision-mocks.d.ts";

export function generate(
  jsonPath = bindingsJsonPath,
  outDir = generatedDir
): { js: string; dts: string; doc: ContextsDocument } {
  const doc = loadContext(jsonPath);
  mkdirSync(outDir, { recursive: true });
  const js = emitJs(doc);
  const dts = emitDts(doc);
  writeFileSync(join(outDir, generatedJsName), js, "utf8");
  writeFileSync(join(outDir, generatedDtsName), dts, "utf8");
  return { js, dts, doc };
}

function countMethods(elements: Element[]): { unique: number; overloads: number } {
  let unique = 0;
  let overloads = 0;
  for (const member of membersOf(elements)) {
    if (member.kind === "method") {
      unique += 1;
      overloads += member.overloads.length;
      continue;
    }
    const nested = countMethods(member.elements);
    unique += nested.unique;
    overloads += nested.overloads;
  }
  return { unique, overloads };
}

function printSummary(doc: ContextsDocument): void {
  let unique = 0;
  let overloads = 0;
  let opaque = 0;
  let scalar = 0;
  for (const binding of doc.bindings) {
    const kind = classifyBinding(binding);
    if (kind === "scalar") {
      scalar += 1;
      continue;
    }
    if (kind === "opaque") {
      opaque += 1;
      continue;
    }
    const counted = countMethods(binding.elements);
    unique += counted.unique;
    overloads += counted.overloads;
  }
  process.stdout.write(
    `wrote ${generatedJsName} and ${generatedDtsName}\n` +
      `${doc.bindings.length} bindings (${scalar} scalar, ${opaque} opaque); ` +
      `${unique} methods (${overloads} signatures including overloads)\n`
  );
}

const entry = process.argv[1];
if (entry && import.meta.url === pathToFileURL(entry).href) {
  const { doc } = generate();
  printSummary(doc);
}
