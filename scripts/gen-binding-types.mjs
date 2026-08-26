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
//
// The `--library-args` mode emits the same interfaces WITHOUT their
// `declare const`s, merged across several artifacts, for library scope: a
// library sees none of the per-context globals and takes them as arguments
// instead, so it needs the argument types without the bindings.
//
// Usage: node scripts/gen-binding-types.mjs --library-args \
//          [--skip <name,name,...>] <artifact.json...> > library-args.d.ts
import { readFileSync } from "node:fs";
import { basename } from "node:path";

const REQUEST_MAPS = new Set(["requestHeaders", "requestParameters", "requestCookies"]);

// Bindings the metadata reports as an `object` with no enumerated members, but
// whose shape is documented and named in nextgen-common.d.ts. Generated
// verbatim they become `declare const requestProperties: object`, and under
// `strict` that is unusable — no member read, no indexing, no autocomplete.
const NAMED_OPAQUE = {
  requestProperties: "RequestProperties",
  clientProperties: "ClientProperties",
};

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

// Fluent builders. The metadata types every chaining method's return as a bare
// `object`, which breaks the ordinary `action.goTo("true").withHeader(…)` —
// each of these returns the builder itself. `true` means every one of the
// interface's `object`-returning methods chains; a list names the ones that do,
// for interfaces whose other `object` returns are real values (`nodeState.get`).
const SELF_RETURNING = {
  Action: true,
  NodeState: ["putShared", "putTransient", "mergeShared", "mergeTransient"],
};
const SELF_RETURNING_HEADER =
  "Fluent builder methods return their own interface, not metadata's bare `object`.";

const selfReturns = (iface, member) => {
  const rule = SELF_RETURNING[iface];
  return rule === true || (Array.isArray(rule) && rule.includes(member));
};

const tsType = (t) =>
  ({ string: "StringLike", number: "number", boolean: "boolean", array: "any[]", void: "void", object: "object" })[t] ??
  "any";
const safe = (n) => (/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(n) ? n : JSON.stringify(n));
const pascal = (n) => n.charAt(0).toUpperCase() + n.slice(1);
const params = (ps, refinement) =>
  ps.map((p) => `${safe(p.name)}: ${refinement?.parameterTypes?.[p.name] ?? tsType(p.javaScriptType)}`);

// Prettier's own rule, applied here so a regenerated file is already formatted:
// one line while it fits in 80 columns, otherwise a parameter per line.
const signature = (indent, head, parts, tail) => {
  const oneLine = `${indent}${head}(${parts.join(", ")})${tail}`;
  if (oneLine.length <= 80) return oneLine;
  const lines = [`${indent}${head}(`];
  parts.forEach((part, i) => lines.push(`${indent}  ${part}${i < parts.length - 1 ? "," : ""}`));
  lines.push(`${indent})${tail}`);
  return lines.join("\n");
};

// One entry per member, so `--library-args` can merge two contexts' views of the
// same binding. Each entry carries its member `name` as well as its rendered
// `text`: the merge partitions on the member, not on the string, because two
// contexts can render the same member differently and a text-only comparison
// would call that a divergence. An entry may span lines (a documented overload,
// a nested field object); the whole entry is the unit.
function body(elements, iface, indent = "  ") {
  const seen = new Set();
  const members = [];
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
        const lines = (overload.comment ?? []).map((comment) => `${indent}// ${comment}`);
        lines.push(signature(indent, safe(el.name), [overload.parameters], `: ${overload.returnType};`));
        members.push({ name: el.name, kind: "method", text: lines.join("\n") });
      }
      const generated = tsType(el.returnType);
      const returnType =
        refinement?.returnType ??
        (generated === "object" && selfReturns(iface, el.name) ? iface : generated);
      const sig = signature(indent, safe(el.name), params(el.parameters, refinement), `: ${returnType};`);
      if (!seen.has(sig)) {
        seen.add(sig);
        members.push({ name: el.name, kind: "method", text: sig });
      }
    } else if (el.elementType === "field") {
      if (el.elements && el.elements.length) {
        members.push({
          name: el.name,
          kind: "field",
          text: [
            `${indent}${safe(el.name)}: {`,
            ...body(el.elements, pascal(el.name), indent + "  ").map((m) => m.text),
            `${indent}};`,
          ].join("\n"),
        });
      } else {
        members.push({ name: el.name, kind: "field", text: `${indent}${safe(el.name)}: ${tsType(el.javaScriptType)};` });
      }
    }
  }
  return members;
}

/** Bindings this artifact contributes an `interface` for, in artifact order. */
const interfaceBindings = (ctx, skip) =>
  ctx.bindings.filter(
    (binding) =>
      !skip.has(binding.name) &&
      !REQUEST_MAPS.has(binding.name) &&
      binding.javaScriptType === "object" &&
      binding.elements?.length
  );

const refinementHeaders = (ifaces) => {
  const headers = ifaces.flatMap((iface) =>
    Object.values(REFINEMENTS[iface] ?? {}).map((refinement) => refinement.header)
  );
  if (ifaces.some((iface) => SELF_RETURNING[iface])) headers.push(SELF_RETURNING_HEADER);
  return [...new Set(headers)];
};

const refinementNotes = (headers) =>
  headers.length ? ["// Applied metadata refinements:", ...headers.map((h) => `//   - ${h}`)] : [];

// A custom access-token field: any JSON-ish scalar, array, or object.
const TOKEN_FIELD_VALUE = [
  "// A custom access-token field: any JSON-ish scalar, array, or object. Whatever",
  "// you set here lands in the CTS entry, or as a JWT claim for client-based",
  "// tokens — keep it small.",
  "type TokenFieldValue = StringLike | number | boolean | any[] | object;",
  "",
];

const emit = (out) => {
  while (out.length && out[out.length - 1] === "") out.pop();
  process.stdout.write(out.join("\n") + "\n");
};

function perContext(jsonPath, skip) {
  const ctx = JSON.parse(readFileSync(jsonPath, "utf8"));
  const ifaces = interfaceBindings(ctx, skip).map((binding) => pascal(binding.name));
  const out = [
    `// GENERATED from docs/api/bindings/${basename(jsonPath)} by`,
    `// scripts/gen-binding-types.mjs — do not edit by hand. Context: ${ctx._id}.`,
    `// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.`,
    ...refinementNotes(refinementHeaders(ifaces)),
    "",
  ];
  if (ifaces.includes("AccessToken")) out.push(...TOKEN_FIELD_VALUE);
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
    if (NAMED_OPAQUE[b.name]) {
      out.push(`declare const ${safe(b.name)}: ${NAMED_OPAQUE[b.name]};`);
      continue;
    }
    if (!b.elements || b.elements.length === 0) {
      out.push(`declare const ${safe(b.name)}: object;`);
      continue;
    }
    const iface = pascal(b.name);
    out.push(`interface ${iface} {`, ...body(b.elements, iface).map((m) => m.text), `}`, `declare const ${safe(b.name)}: ${iface};`, "");
  }
  emit(out);
}

// A short type-name prefix for a context, from its artifact filename:
// `oauth2-jwt-issuer-next.json` -> `Oauth2JwtIssuer`.
const contextPrefix = (jsonPath) =>
  basename(jsonPath)
    .replace(/(-next)?\.json$/, "")
    .split("-")
    .map(pascal)
    .join("");

function libraryArgs(jsonPaths, skip) {
  // iface -> {byContext: Map<prefix, Map<memberName, text[]>>, order: [], contexts: []}
  const merged = new Map();
  for (const jsonPath of jsonPaths) {
    const ctx = JSON.parse(readFileSync(jsonPath, "utf8"));
    const prefix = contextPrefix(jsonPath);
    for (const binding of interfaceBindings(ctx, skip)) {
      const iface = pascal(binding.name);
      if (!merged.has(iface)) merged.set(iface, { byContext: new Map(), order: [], contexts: [] });
      const entry = merged.get(iface);
      entry.contexts.push(ctx._id);
      const members = new Map();
      for (const member of body(binding.elements, iface)) {
        if (!members.has(member.name)) {
          members.set(member.name, { kind: member.kind, texts: [] });
          if (!entry.order.includes(member.name)) entry.order.push(member.name);
        }
        const entryMember = members.get(member.name);
        entryMember.kind = member.kind;
        if (!entryMember.texts.includes(member.text)) entryMember.texts.push(member.text);
      }
      entry.byContext.set(prefix, members);
    }
  }

  // Each binding is the members EVERY contributing context has. Unioning them
  // was unsound: only the JWT-issuer context's `idRepository` has `createUser`,
  // and a merged `IdRepository` let a library call it on the scripted-decision
  // binding, which has `getIdentity` and nothing else — a type-checked "not a
  // function" at runtime.
  //
  // Members only some contexts have are OMITTED, and named in a comment above
  // the interface. A context-qualified type carrying them was the other option
  // and is worse: a caller resolves `require()` through its own leaf, which does
  // NOT include this file, so a library annotated with such a name fails to
  // compile in every caller — the whole module, not just that export.
  const out = [
    "// GENERATED by scripts/gen-binding-types.mjs --library-args — do not edit by",
    "// hand. See the regenerate command at the bottom of this file.",
    "//",
    "// Types a caller can hand a library script. A library sees none of the",
    "// per-context globals (verified 2026-07-29 — docs/api/12-script-bindings-matrix.md),",
    "// so it takes them as arguments, and the argument types have to exist in library",
    "// scope or the factory signature cannot be written.",
    "//",
    "// Each binding is the members that EVERY next-gen context able to require() a",
    "// library agrees on — what a library can call whoever hands it the value. A",
    "// member only some contexts have is omitted and named in a comment; a library",
    "// that needs one declares a module-local structural type or casts, which makes",
    "// the context it is written for explicit rather than ambient.",
    "//",
    "// A caller re-checks this library against its OWN same-named types, so an",
    "// overlay declaring one has to stay a compatible refinement of the surface",
    "// here: extra members and more precise returns are fine, narrower parameters",
    "// and incompatible returns are not.",
    "//",
    "// `NodeState` is NOT here — library.d.ts hand-writes a better one.",
  ];
  const ifaces = [...merged.keys()].sort();
  const notes = refinementNotes(refinementHeaders(ifaces));
  out.push(...notes, "");
  if (merged.has("AccessToken")) out.push(...TOKEN_FIELD_VALUE);

  for (const iface of ifaces) {
    const entry = merged.get(iface);
    const contexts = [...entry.byContext.keys()];
    // Common means every context has the member AND describes it the same way.
    // Name alone is not enough: two contexts rendering `lookup(x)` with
    // different return types would both be emitted, and overload order — file
    // order, not intent — would pick one. A member that differs is as
    // uncallable as one that is missing, so it is omitted the same way.
    const common = entry.order.filter((name) => {
      const members = contexts.map((prefix) => entry.byContext.get(prefix).get(name));
      if (members.some((member) => !member)) return false;
      const first = JSON.stringify([members[0].kind, members[0].texts]);
      return members.every((member) => JSON.stringify([member.kind, member.texts]) === first);
    });
    // An empty common surface would be an interface that accepts nearly any
    // object. Say so and emit only the qualified types.
    const omitted = [];
    for (const prefix of contexts) {
      const members = entry.byContext.get(prefix);
      for (const name of entry.order) {
        if (!members.has(name) || common.includes(name)) continue;
        for (const text of members.get(name).texts) {
          omitted.push(`//   ${text.trim().replace(/\s*\n\s*/g, " ")}   (${prefix} only)`);
        }
      }
    }
    if (omitted.length) {
      out.push("// Omitted — not on every context's binding, so a library cannot call it", ...omitted);
    }
    if (common.length === 0) {
      out.push(
        `// ${entry.contexts.join(", ")}`,
        `// No member is common to every context, so there is no \`${iface}\` at all:`,
        "// an empty interface would accept very nearly any object.",
        ""
      );
    } else {
      out.push(`// ${entry.contexts.join(", ")}`, `interface ${iface} {`);
      // Every context renders these identically — that is what made them
      // common — so any context's rendering is the rendering.
      for (const name of common) out.push(...entry.byContext.get(contexts[0]).get(name).texts);
      out.push(`}`, "");
    }

  }

  out.push(
    "// Regenerate:",
    "//   node scripts/gen-binding-types.mjs --library-args \\",
    `//     --skip ${[...skip].join(",")} \\`,
    ...jsonPaths.map((p, i) => `//     ${p}${i === jsonPaths.length - 1 ? "" : " \\"}`),
    "//   > src/scripts/templates/am/types/library-args.d.ts",
    "//",
    "// The output is already prettier-formatted (`signature` applies prettier's",
    "// 80-column rule), so a regenerate is a clean diff or no diff at all."
  );
  emit(out);
}

const argv = process.argv.slice(2);
if (argv[0] === "--library-args") {
  const rest = argv.slice(1);
  const jsonPaths = [];
  let skip = new Set();
  for (let i = 0; i < rest.length; i++) {
    if (rest[i] === "--skip") skip = new Set(rest[++i].split(","));
    else jsonPaths.push(rest[i]);
  }
  libraryArgs(jsonPaths, skip);
} else {
  const [jsonPath, ...exclude] = argv;
  perContext(jsonPath, new Set(exclude));
}
