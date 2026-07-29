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

// Ping's editor metadata loses a little useful type information. Keep every
// correction here, keyed by generated interface + member, so each overlay that
// exposes one of these bindings gets the same faithful declaration.
const REFINEMENTS = {
  // Token field get/set are Java `Object` in and out. Generated verbatim, the
  // `object` param type rejects the ordinary `setField("claim", "a string")`
  // call and the `object` return makes a read unusable without a cast.
  AccessToken: {
    getField: {
      returnType: "any",
      header: "`AccessToken.getField` returns `any`, not metadata's bare `object`.",
    },
    setField: {
      parameterTypes: { value: "TokenFieldValue" },
      header:
        "`AccessToken.setField` accepts `TokenFieldValue`, not metadata's bare `object`.",
    },
  },
  // The may-act `token` binding is a 5-member subset of the same wrapper.
  Token: {
    getField: {
      returnType: "any",
      header: "`Token.getField` returns `any`, not metadata's bare `object`.",
    },
  },
  Identity: {
    getAttributeValues: {
      before: "getUniversalId",
      overloads: [
        {
          comment: [
            "Typed managed-user attribute names first (autocomplete; docs/api/14), then",
            "the permissive StringLike fallback for any other attribute.",
          ],
          parameters: "attributeName: AmUserAttribute",
          returnType: "any[]",
        },
      ],
      header:
        "`Identity.getAttributeValues` adds the typed `AmUserAttribute` overload first.",
    },
  },
};

const tsType = (t) =>
  ({ string: "StringLike", number: "number", boolean: "boolean", array: "any[]", void: "void", object: "object" })[t] ??
  "any";
const safe = (n) => (/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(n) ? n : JSON.stringify(n));
const pascal = (n) => n.charAt(0).toUpperCase() + n.slice(1);
const params = (ps, refinement) =>
  ps
    .map((p) => `${safe(p.name)}: ${refinement?.parameterTypes?.[p.name] ?? tsType(p.javaScriptType)}`)
    .join(", ");

function body(elements, iface, indent = "  ") {
  const seen = new Set();
  const lines = [];
  const ordered = [...elements];
  for (const [name, refinement] of Object.entries(REFINEMENTS[iface] ?? {})) {
    if (!refinement.before) continue;
    const from = ordered.findIndex((element) => element.name === name);
    const to = ordered.findIndex((element) => element.name === refinement.before);
    if (from >= 0 && to >= 0 && from > to) ordered.splice(to, 0, ordered.splice(from, 1)[0]);
  }
  for (const el of ordered) {
    if (el.elementType === "method") {
      const refinement = REFINEMENTS[iface]?.[el.name];
      for (const overload of refinement?.overloads ?? []) {
        for (const comment of overload.comment ?? []) lines.push(`${indent}// ${comment}`);
        lines.push(`${indent}${safe(el.name)}(${overload.parameters}): ${overload.returnType};`);
      }
      const sig = `${indent}${safe(el.name)}(${params(el.parameters, refinement)}): ${refinement?.returnType ?? tsType(el.returnType)};`;
      if (!seen.has(sig)) {
        seen.add(sig);
        lines.push(sig);
      }
    } else if (el.elementType === "field") {
      if (el.elements && el.elements.length) {
        lines.push(`${indent}${safe(el.name)}: {`, body(el.elements, pascal(el.name), indent + "  "), `${indent}};`);
      } else {
        lines.push(`${indent}${safe(el.name)}: ${tsType(el.javaScriptType)};`);
      }
    }
  }
  return lines.join("\n");
}

const emittedInterfaces = ctx.bindings
  .filter((binding) => !skip.has(binding.name) && binding.javaScriptType === "object" && binding.elements?.length)
  .map((binding) => pascal(binding.name));
const appliedRefinements = emittedInterfaces.flatMap((iface) =>
  Object.values(REFINEMENTS[iface] ?? {}).map((refinement) => refinement.header)
);

const out = [
  `// GENERATED from docs/api/bindings/${basename(jsonPath)} by`,
  `// scripts/gen-binding-types.mjs — do not edit by hand. Context: ${ctx._id}.`,
  `// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.`,
  ...(appliedRefinements.length
    ? ["// Applied metadata refinements:", ...appliedRefinements.map((refinement) => `//   - ${refinement}`)]
    : []),
  "",
];
if (emittedInterfaces.includes("AccessToken")) {
  out.push(
    "// A custom access-token field: any JSON-ish scalar, array, or object. Whatever",
    "// you set here lands in the CTS entry, or as a JWT claim for client-based",
    "// tokens — keep it small.",
    "type TokenFieldValue = StringLike | number | boolean | any[] | object;",
    ""
  );
}
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
  out.push(`interface ${iface} {`, body(b.elements, iface), `}`, `declare const ${safe(b.name)}: ${iface};`, "");
}
while (out.length && out[out.length - 1] === "") out.pop();
process.stdout.write(out.join("\n") + "\n");
