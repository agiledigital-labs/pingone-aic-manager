import { assertIdent } from "./idents.ts";
import { classifyBinding, membersOf, overloadLabel } from "./schema.ts";
import type { Binding, ContextsDocument, Element, MethodElement } from "./schema.ts";

export const UNSEEDED_STRING = "__rhino-local-unseeded__";

const HEADER = `\
// GENERATED from docs/api/bindings/scripted-decision-next.json — do not edit.
// Re-run: npm --prefix scripts/rhino-local/ts run generate
//
// Evaluated by AM's Rhino 1.7.14 as part of the script-under-test's scope.
// AM-safe JavaScript only: no let, no top-level const, no for...of, no object
// shorthand, no destructuring, no default parameters, and none of Map / Set /
// Symbol / Promise / WeakMap / WeakSet. See docs/api/12-script-bindings-matrix.md.
`;

const HELPERS = `\
function __rhinoLocalArgKind(value) {
  if (Array.isArray(value)) {
    return "array";
  }
  var t = typeof value;
  if (t === "string" || t === "number" || t === "boolean") {
    return t;
  }
  return "object";
}

function __rhinoLocalNotMocked(binding, method, args, signatures) {
  var arity = args.length;
  var matched = [];
  var i;
  var j;
  var sig;
  var typesOk;
  for (i = 0; i < signatures.length; i += 1) {
    sig = signatures[i];
    if (sig.arity !== arity) {
      continue;
    }
    typesOk = true;
    for (j = 0; j < arity; j += 1) {
      if (__rhinoLocalArgKind(args[j]) !== sig.types[j]) {
        typesOk = false;
        break;
      }
    }
    if (typesOk) {
      matched.push(sig.label);
    }
  }
  if (matched.length === 0) {
    for (i = 0; i < signatures.length; i += 1) {
      if (signatures[i].arity === arity) {
        matched.push(signatures[i].label);
      }
    }
  }
  if (matched.length === 0) {
    matched.push("no matching signature");
  }
  throw new Error(
    "rhino-local: not mocked: " +
      binding +
      "." +
      method +
      " arity=" +
      arity +
      " overload=[" +
      matched.join(" | ") +
      "]"
  );
}
`;

export function emitJs(doc: ContextsDocument): string {
  const lines: string[] = [HEADER, HELPERS];
  for (const binding of doc.bindings) {
    lines.push(emitBinding(binding));
  }
  return lines.join("\n").replace(/\n+$/, "") + "\n";
}

function emitBinding(binding: Binding): string {
  const name = assertIdent(binding.name, "binding name");
  const kind = classifyBinding(binding);
  if (kind === "scalar") {
    return `var ${name} = ${scalarLiteral(binding)};\n`;
  }
  if (kind === "opaque") {
    return (
      `// Opaque container: the contexts metadata lists no elements. A case\n` +
      `// seeds this object directly; a later slice may replace the assignment.\n` +
      `var ${name} = {};\n`
    );
  }
  return `var ${name} = ${emitObject(binding.elements, name, 0)};\n`;
}

function scalarLiteral(binding: Binding): string {
  switch (binding.javaScriptType) {
    case "string":
      return JSON.stringify(UNSEEDED_STRING);
    case "boolean":
      return "false";
    case "number":
      return "NaN";
    default:
      throw new Error(
        `rhino-local: not a scalar javaScriptType: ${binding.javaScriptType}`
      );
  }
}

function emitObject(elements: Element[], path: string, indent: number): string {
  const members = membersOf(elements);
  if (members.length === 0) {
    return "{}";
  }
  const pad = "  ".repeat(indent);
  const inner = "  ".repeat(indent + 1);
  const parts: string[] = ["{"];
  for (const member of members) {
    const ident = assertIdent(member.name, `member of ${path}`);
    if (member.kind === "field") {
      const nestedPath = `${path}.${member.name}`;
      const value =
        member.elements.length === 0
          ? "{}"
          : emitObject(member.elements, nestedPath, indent + 1);
      parts.push(`${inner}${ident}: ${value},`);
      continue;
    }
    parts.push(`${inner}${ident}: ${emitMethod(path, member.overloads, indent + 1)},`);
  }
  parts.push(`${pad}}`);
  return parts.join("\n");
}

function emitMethod(
  bindingPath: string,
  overloads: MethodElement[],
  indent: number
): string {
  const first = overloads[0];
  if (!first) {
    throw new Error(`rhino-local: method at ${bindingPath} has no overloads`);
  }
  const bodyPad = "  ".repeat(indent + 1);
  const closePad = "  ".repeat(indent);
  const sigPad = "  ".repeat(indent + 2);
  const sigs = overloads.map((overload) => {
    const arity = overload.parameters.length;
    const types = JSON.stringify(
      overload.parameters.map((parameter) => parameter.javaScriptType)
    );
    const label = JSON.stringify(overloadLabel(overload));
    return `{ arity: ${arity}, types: ${types}, label: ${label} }`;
  });
  const firstSig = sigs[0];
  const sigBlock =
    sigs.length === 1 && firstSig
      ? `[${firstSig}]`
      : `[\n${sigs.map((sig) => `${sigPad}${sig},`).join("\n")}\n${bodyPad}]`;
  return [
    "function () {",
    `${bodyPad}__rhinoLocalNotMocked(${JSON.stringify(bindingPath)}, ${JSON.stringify(first.name)}, arguments, ${sigBlock});`,
    `${closePad}}`,
  ].join("\n");
}
