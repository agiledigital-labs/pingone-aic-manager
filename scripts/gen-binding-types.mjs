// Generate a per-context overlay .d.ts from a script-context binding artifact
// (docs/api/bindings/*.json — see docs/api/13-script-contexts.md).
//
// Emits `declare const` + interfaces for every binding EXCEPT the shared
// next-gen-common set (passed as args), which is provided by common.d.ts +
// nextgen-common.d.ts. Map-like bindings with no enumerated methods
// (requestHeaders/Parameters/Cookies) reference the shared `RequestMap` type
// from rhino-1.7.14.d.ts.
//
// Usage: node scripts/gen-binding-types.mjs <artifact.json> <exclude...> > out.d.ts
import { readFileSync } from "node:fs";
import { basename } from "node:path";

const [, , jsonPath, ...exclude] = process.argv;
const skip = new Set(exclude);
const ctx = JSON.parse(readFileSync(jsonPath, "utf8"));
const REQUEST_MAPS = new Set(["requestHeaders", "requestParameters", "requestCookies"]);

const tsType = (t) =>
  ({ string: "StringLike", number: "number", boolean: "boolean", array: "any[]", void: "void", object: "object" })[t] ??
  "any";
const safe = (n) => (/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(n) ? n : JSON.stringify(n));
const pascal = (n) => n.charAt(0).toUpperCase() + n.slice(1);
const params = (ps) => ps.map((p) => `${safe(p.name)}: ${tsType(p.javaScriptType)}`).join(", ");

function body(elements, indent = "  ") {
  const seen = new Set();
  const lines = [];
  for (const el of elements) {
    if (el.elementType === "method") {
      const sig = `${indent}${safe(el.name)}(${params(el.parameters)}): ${tsType(el.returnType)};`;
      if (!seen.has(sig)) {
        seen.add(sig);
        lines.push(sig);
      }
    } else if (el.elementType === "field") {
      if (el.elements && el.elements.length) {
        lines.push(`${indent}${safe(el.name)}: {`, body(el.elements, indent + "  "), `${indent}};`);
      } else {
        lines.push(`${indent}${safe(el.name)}: ${tsType(el.javaScriptType)};`);
      }
    }
  }
  return lines.join("\n");
}

const out = [
  `// GENERATED from docs/api/bindings/${basename(jsonPath)} by`,
  `// scripts/gen-binding-types.mjs — do not edit by hand. Context: ${ctx._id}.`,
  `// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.`,
  "",
];
for (const b of ctx.bindings) {
  if (skip.has(b.name)) continue;
  if (b.javaScriptType !== "object") {
    out.push(`declare const ${safe(b.name)}: ${tsType(b.javaScriptType)};`);
    continue;
  }
  if (REQUEST_MAPS.has(b.name)) {
    out.push(`declare const ${b.name}: RequestMap;`);
    continue;
  }
  if (!b.elements || b.elements.length === 0) {
    out.push(`declare const ${safe(b.name)}: object;`);
    continue;
  }
  const iface = pascal(b.name);
  out.push(`interface ${iface} {`, body(b.elements), `}`, `declare const ${safe(b.name)}: ${iface};`, "");
}
process.stdout.write(out.join("\n") + "\n");
