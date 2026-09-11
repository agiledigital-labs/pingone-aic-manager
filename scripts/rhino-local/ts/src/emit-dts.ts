import { assertIdent, pascal, tsParamName, tsType } from "./idents.ts";
import { classifyBinding, membersOf } from "./schema.ts";
import type { ContextsDocument, Element, MethodElement } from "./schema.ts";

const HEADER = `\
// GENERATED from docs/api/bindings/scripted-decision-next.json — do not edit.
// Re-run: npm --prefix scripts/rhino-local/ts run generate
//
// Case-authoring types for the scripted-decision mock surface. javaScriptType
// values are mapped as-is (object → object, array → unknown[]); they do not
// include the Rhino Java interop layer in src/scripts/templates/am/types/.
// Every generated method throws at runtime until a later slice implements it.
`;

export function emitDts(doc: ContextsDocument): string {
  const interfaces: string[] = [];
  const aggregate: string[] = ["export interface ScriptedDecisionMocks {"];

  for (const binding of doc.bindings) {
    const name = assertIdent(binding.name, "binding name");
    const kind = classifyBinding(binding);
    if (kind === "scalar") {
      aggregate.push(`  ${name}: ${tsType(binding.javaScriptType)};`);
      continue;
    }
    if (kind === "opaque") {
      aggregate.push(`  ${name}: OpaqueBinding;`);
      continue;
    }
    const iface = pascal(name);
    interfaces.push(...emitInterface(iface, binding.elements, iface));
    aggregate.push(`  ${name}: ${iface};`);
  }

  aggregate.push("}");

  const opaque = [
    "/**",
    " * Binding whose contexts metadata lists an empty `elements` array.",
    " * A test case seeds this object directly; the generator does not invent",
    " * methods the JSON does not name.",
    " */",
    "export type OpaqueBinding = {",
    "  [key: string]: unknown;",
    "};",
    "",
  ];

  return [HEADER, ...opaque, ...interfaces, ...aggregate, ""].join("\n");
}

function emitInterface(
  ifaceName: string,
  elements: Element[],
  typePrefix: string
): string[] {
  const members = membersOf(elements);
  const nested: string[] = [];
  const body: string[] = [`export interface ${ifaceName} {`];

  for (const member of members) {
    const ident = assertIdent(member.name, `member of ${ifaceName}`);
    if (member.kind === "field") {
      const nestedName = `${typePrefix}${pascal(member.name)}`;
      if (member.elements.length === 0) {
        body.push(`  ${ident}: OpaqueBinding;`);
      } else {
        nested.push(...emitInterface(nestedName, member.elements, nestedName));
        body.push(`  ${ident}: ${nestedName};`);
      }
      continue;
    }
    for (const overload of member.overloads) {
      body.push(methodSignature(ident, overload));
    }
  }

  body.push("}", "");
  return [...nested, ...body];
}

function methodSignature(name: string, overload: MethodElement): string {
  const params = overload.parameters.map(
    (parameter) => `${tsParamName(parameter.name)}: ${tsType(parameter.javaScriptType)}`
  );
  const ret = tsType(overload.returnType);
  const oneLine = `  ${name}(${params.join(", ")}): ${ret};`;
  if (oneLine.length <= 80) {
    return oneLine;
  }
  const lines = [`  ${name}(`];
  params.forEach((param, index) => {
    const comma = index < params.length - 1 ? "," : "";
    lines.push(`    ${param}${comma}`);
  });
  lines.push(`  ): ${ret};`);
  return lines.join("\n");
}
