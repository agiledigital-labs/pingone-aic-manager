// Generate a per-context overlay .d.ts from a script-context binding artifact
// (docs/api/bindings/*.json — see docs/api/13-script-contexts.md).
//
// Emits `declare const` + interfaces for every binding EXCEPT the shared
// next-gen-common set (passed as args), which is provided by common.d.ts +
// nextgen-common.d.ts. Map-like bindings with no enumerated methods reference
// the shared types in rhino-1.7.14.d.ts: requestHeaders/Parameters are the
// multimap `RequestMap`, requestCookies the single-valued `RequestCookieMap`.
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

const REQUEST_MAPS = new Map([
  ["requestHeaders", "RequestMap"],
  ["requestParameters", "RequestMap"],
  ["requestCookies", "RequestCookieMap"],
]);

// Bindings the metadata reports as an `object` with no enumerated members, but
// whose shape is documented and named in nextgen-common.d.ts. Generated
// verbatim they become `declare const requestProperties: object`, and under
// `strict` that is unusable — no member read, no indexing, no autocomplete.
const NAMED_OPAQUE = {
  requestProperties: "RequestProperties",
  clientProperties: "ClientProperties",
};

const NULLABLE_TOKEN_GETTERS =
  "`AccessToken` getters that answer JS `null` on a token with no value return `T | null`.";
const NULLABLE_MAY_ACT =
  "`Token.getAct` / `getMayAct` return `object | null`: an absent claim is JS `null`.";

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
    // Each of these answers JS `null` on a `client_credentials` token that
    // carries no value for it (measured 2026-11-10, docs/api/12), so an
    // unguarded read is a runtime failure the bare return type hid.
    getPermissions: { returnType: "object | null", header: NULLABLE_TOKEN_GETTERS },
    getNonce: { returnType: "StringLike | null", header: NULLABLE_TOKEN_GETTERS },
    getAct: { returnType: "object | null", header: NULLABLE_TOKEN_GETTERS },
    getMayAct: { returnType: "object | null", header: NULLABLE_TOKEN_GETTERS },
    getAuthLevel: { returnType: "number | null", header: NULLABLE_TOKEN_GETTERS },
    getClaims: { returnType: "StringLike | null", header: NULLABLE_TOKEN_GETTERS },
    getConfirmationKey: { returnType: "object | null", header: NULLABLE_TOKEN_GETTERS },
    // `("k", "<json>")` is the measured call on next-gen; the object form is
    // metadata's and was not measured.
    addExtraJsonData: {
      parameterTypes: { value: "StringLike | object" },
      header:
        "`AccessToken.addExtraJsonData` also accepts the JSON string measured on next-gen.",
    },
    // The `scopes` binding is a Java list, and the token copies what it is
    // given: a JS array and the `scopes` list are both measured.
    setScope: {
      parameterTypes: { scopes: "any[] | ScopeList" },
      header: "`AccessToken.setScope` accepts a JS array or the `scopes` list.",
    },
  },
  // The may-act `token` binding is a 5-member subset of the same wrapper.
  Token: {
    getField: {
      returnType: "any",
      header: "`Token.getField` returns `any`, not metadata's bare `object`.",
    },
    // An absent claim, and JSON null, are both JS `null` (measured 2026-11-10).
    getAct: { returnType: "object | null", header: NULLABLE_MAY_ACT },
    getMayAct: { returnType: "object | null", header: NULLABLE_MAY_ACT },
    // `setMayAct(null)` is accepted and stamps JSON null (measured 2026-11-10).
    setMayAct: {
      parameterTypes: { value: "object | null" },
      header: "`Token.setMayAct` also accepts `null`, which stamps JSON null.",
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

// Per-context corrections, keyed by context id. Each is something a live script
// on the sandbox (2026-11-10, docs/api/12) contradicted in the descriptor, so it
// belongs to the context rather than to a shared interface:
//   types      binding -> named type, for a binding the descriptor leaves as a bare
//              `array` / `object` / `unknown`
//   omit       interface -> members the runtime refuses, left undeclared
//   returns    interface -> member -> return type
//   params     interface -> member -> parameter name -> type
//   notes      lines for the "Applied metadata refinements" header
//   preamble   declarations the `types` above name (emitted first)
//   trailer    declarations appended after the bindings
//   noSession  token modification / OIDC claims: `session` is NOT bound
const SESSION_UNBOUND = [
  "// `session` is deliberately not declared. Ping documents it for this context, the",
  "// descriptor omits it, and the runtime does not bind it: reading it bare is",
  "// `ReferenceError: \"session\" is not defined.` (measured 2026-11-10, docs/api/12).",
  "// Declared as nullable it would let `session != null` compile and then throw. Left",
  "// undeclared, a bare read and a `typeof session` probe are both type errors.",
];
const EXISTING_SESSION_GUARDED = [
  "// `existingSession` is listed by the descriptor but is a ReferenceError on a request",
  "// with no session cookie (measured 2026-11-10); with one it is unmeasured here.",
  "// Guard with `typeof existingSession !== \"undefined\"` before reading it.",
];
const CONTEXTS = {
  OAUTH2_ACCESS_TOKEN_MODIFICATION_NEXT_GEN: {
    types: { scopes: "ScopeList" },
    noSession: true,
    notes: ["`scopes` is a `ScopeList` (a Java list), not a JS array."],
  },
  OAUTH2_MAY_ACT_NEXT_GEN: {
    types: { scopes: "ScopeList" },
    notes: ["`scopes` is a `ScopeList` (a Java list), not a JS array."],
  },
  OIDC_CLAIMS_NEXT_GEN: {
    types: { scopes: "ScopeList", claims: "OidcClaimsMap" },
    noSession: true,
    notes: [
      "`scopes` is a `ScopeList` (a Java list), not a JS array.",
      "`claims` is an `OidcClaimsMap`, and the script's result is an `OidcClaimsResult`.",
    ],
    preamble: [
      "// The `claims` binding: a copy of AM's `{ sub, subname }`, both the user's id.",
      "// Changing it changes nothing unless the script returns it (measured 2026-11-10,",
      "// docs/api/12). Values read back as JS strings, so `claims.sub === \"…\"` holds;",
      "// `delete claims.subname` is `false` and leaves the key.",
      "interface OidcClaimsMap {",
      "  get(key: StringLike): unknown;",
      "  put(key: StringLike, value: unknown): unknown;",
      "  remove(key: StringLike): unknown;",
      "  containsKey(key: StringLike): boolean;",
      "  /** Absent once `remove(\"sub\")` has run. */",
      "  sub?: string;",
      "  subname?: string;",
      "  [key: string]: unknown;",
      "}",
      "",
      "// What AM does with the script: the COMPLETION VALUE (its last expression), not a",
      "// `return`. `{ values, compositeScopes? }` is accepted and its claims emitted; a plain",
      "// object with no `values`, or `undefined`, gives no script claims; a throw is a 500.",
      "// Numbers come back as doubles (`42` is `42.0`). `compositeScopes` filters nothing.",
      "// Annotate with `/** @type {OidcClaimsResult} */`, or end the script with the literal.",
      "interface OidcClaimsResult {",
      "  values: { [claim: string]: unknown };",
      "  compositeScopes?: { [scope: string]: unknown };",
      "}",
      "",
    ],
  },
  OAUTH2_VALIDATE_SCOPE_NEXT_GEN: {
    types: {
      scopeValidatorHelper: "ScopeValidatorHelper",
      requestedScopes: "ValidateScopeList",
      availableScopes: "ValidateScopeList",
      allowedScopes: "ValidateScopeList",
      defaultScopes: "ValidateScopeList",
    },
    notes: [
      "The four scope sets are `ValidateScopeList`s (Java lists), not JS arrays.",
      "`scopeValidatorHelper` is a `ScopeValidatorHelper`, not `any`.",
    ],
    preamble: [
      "// AM calls the global `validateAccessTokenScope()` with NO arguments after the body",
      "// runs, and takes the JS array of scope strings it returns as the grant (measured",
      "// 2026-11-10, docs/api/12). Those four sets are Java lists of JS strings: `map`,",
      "// `join` and the other Array methods throw `Cannot find function`, and `iterator()`",
      "// is the class shutter. `[].concat(list)` copies one into a JS array (measured).",
      "interface ValidateScopeList extends ScopeList, OpenidmJavaList<string> {",
      "  indexOf(scope: StringLike): number;",
      "  isEmpty(): boolean;",
      "  forEach(callback: (scope: string) => void): void;",
      "  toArray(): JavaArray<string>;",
      "  /** Grows the list in place; `true` on success. */",
      "  add(scope: StringLike): boolean;",
      "}",
      "",
      "interface ScopeValidatorHelper {",
      "  /**",
      "   * Always throws: `400 invalid_scope` with the message as `error_description`.",
      "   * A number is stringified, `null` gives AM's default message, and no argument",
      "   * at all is a `500`.",
      "   */",
      "  throwInvalidScope(message: StringLike | number | null): never;",
      "  inheritAccessTokenScopesOnRefresh(): void;",
      "}",
      "",
    ],
  },
  SOCIAL_IDP_PROFILE_TRANSFORMATION_NEXT_GEN: {
    types: { rawProfile: "RawProfile", existingSession: "object | undefined" },
    returns: { NodeState: { get: "any" } },
    params: { NodeState: { putShared: { value: "any" }, putTransient: { value: "any" } } },
    omit: { NodeState: ["keys"] },
    notes: [
      "`rawProfile` is a `RawProfile`; `nodeState.get` returns `any`, `put*` take `any`, and `keys` is not declared.",
      "NodeState members other than `get`, `putShared`, `putTransient` are the descriptor's, unmeasured here.",
    ],
    preamble: [
      "// The provider's profile: a Java map whose values read back as JS values. `.sub` and",
      "// `get(\"sub\")` are JS strings, `get(missing)` is `null`, `for...in` lists the keys.",
      "// `getObject` and `isDefined` are not functions (measured 2026-11-10, docs/api/12).",
      "// The base tsconfig forbids dot access to an index-signature key, so write",
      "// `rawProfile[\"sub\"]` or `rawProfile.get(\"sub\")`.",
      "interface RawProfile {",
      "  get(key: StringLike): unknown;",
      "  keys(): MeasuredJavaList<unknown>;",
      "  size(): number;",
      "  [key: string]: unknown;",
      "}",
      "",
    ],
    guards: EXISTING_SESSION_GUARDED,
    trailer: [
      "// The result is the script's COMPLETION VALUE: a plain object (`{}` too) is handed to",
      "// the handler; `undefined`, `null` or a string fails the journey (401). Numbers become",
      "// doubles, and an `undefined` top-level property arrives as `null`.",
    ],
  },
  CONFIG_PROVIDER_NODE_NEXT_GEN: {
    types: { existingSession: "object | undefined" },
    returns: { NodeState: { get: "any" } },
    params: { NodeState: { putShared: { value: "any" }, putTransient: { value: "any" } } },
    omit: { NodeState: ["keys"] },
    notes: [
      "`nodeState.get` returns `any`, `put*` take `any`, and `keys` is not declared (it throws on next-gen).",
      "NodeState members other than `get`, `isDefined`, `putShared`, `putTransient` are the descriptor's, unmeasured here.",
    ],
    guards: EXISTING_SESSION_GUARDED,
    trailer: [
      "// AM reads the script's `config` global AFTER the script ends, and the wrapped node",
      "// is configured from it. A plain object is accepted (`{}` gives the node's defaults);",
      "// never assigned, `undefined`, `null`, a string, a number, a boolean, an array or a",
      "// function is a 500 with an empty body. `config` is a ReferenceError until assigned",
      "// (measured 2026-11-10, docs/api/12). Numbers become doubles, and `new",
      "// java.util.HashMap()` is not available on next-gen.",
      "interface ConfigProviderConfig {",
      "  [key: string]: any;",
      "}",
      "declare let config: ConfigProviderConfig | undefined;",
    ],
  },
};

const tsType = (t) =>
  ({ string: "StringLike", number: "number", boolean: "boolean", array: "any[]", void: "void", object: "object" })[t] ??
  "any";
const safe = (n) => (/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(n) ? n : JSON.stringify(n));
const pascal = (n) => n.charAt(0).toUpperCase() + n.slice(1);
const params = (ps, refinement, override) =>
  ps.map(
    (p) => `${safe(p.name)}: ${override?.[p.name] ?? refinement?.parameterTypes?.[p.name] ?? tsType(p.javaScriptType)}`
  );

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
function body(elements, iface, indent = "  ", cfg = {}) {
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
    if (cfg.omit?.[iface]?.includes(el.name)) continue;
    if (el.elementType === "method") {
      const refinement = REFINEMENTS[iface]?.[el.name];
      for (const overload of refinement?.overloads ?? []) {
        const lines = (overload.comment ?? []).map((comment) => `${indent}// ${comment}`);
        lines.push(signature(indent, safe(el.name), [overload.parameters], `: ${overload.returnType};`));
        members.push({ name: el.name, kind: "method", text: lines.join("\n") });
      }
      const generated = tsType(el.returnType);
      const returnType =
        cfg.returns?.[iface]?.[el.name] ??
        refinement?.returnType ??
        (generated === "object" && selfReturns(iface, el.name) ? iface : generated);
      const sig = signature(indent, safe(el.name), params(el.parameters, refinement, cfg.params?.[iface]?.[el.name]), `: ${returnType};`);
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
            ...body(el.elements, pascal(el.name), indent + "  ", cfg).map((m) => m.text),
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
  const cfg = CONTEXTS[ctx._id] ?? {};
  const ifaces = interfaceBindings(ctx, skip).map((binding) => pascal(binding.name));
  const out = [
    `// GENERATED from docs/api/bindings/${basename(jsonPath)} by`,
    `// scripts/gen-binding-types.mjs — do not edit by hand. Context: ${ctx._id}.`,
    `// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.`,
    ...refinementNotes([...refinementHeaders(ifaces), ...(cfg.notes ?? [])]),
    "",
  ];
  if (ifaces.includes("AccessToken")) out.push(...TOKEN_FIELD_VALUE);
  out.push(...(cfg.preamble ?? []));
  for (const b of ctx.bindings) {
    if (skip.has(b.name)) continue;
    if (cfg.types?.[b.name]) {
      if (b.name === "existingSession") out.push(...(cfg.guards ?? []));
      out.push(`declare const ${safe(b.name)}: ${cfg.types[b.name]};`);
      continue;
    }
    if (b.javaScriptType !== "object") {
      out.push(`declare const ${safe(b.name)}: ${tsType(b.javaScriptType)};`);
      continue;
    }
    if (REQUEST_MAPS.has(b.name)) {
      out.push(`declare const ${b.name}: ${REQUEST_MAPS.get(b.name)};`);
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
    out.push(`interface ${iface} {`, ...body(b.elements, iface, "  ", cfg).map((m) => m.text), `}`, `declare const ${safe(b.name)}: ${iface};`, "");
  }
  if (cfg.noSession) out.push("", ...SESSION_UNBOUND);
  if (cfg.trailer) out.push("", ...cfg.trailer);
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
