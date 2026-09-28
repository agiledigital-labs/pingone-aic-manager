const IDENT = /^[A-Za-z_$][A-Za-z0-9_$]*$/;

const TS_PARAM_RENAMES: Record<string, string> = {
  arguments: "args",
};

export function assertIdent(name: string, what: string): string {
  if (!IDENT.test(name)) {
    throw new Error(`rhino-local: ${what} is not a JS identifier: ${JSON.stringify(name)}`);
  }
  return name;
}

export function pascal(name: string): string {
  const ident = assertIdent(name, "type name");
  const first = ident.charAt(0);
  if (!first) {
    throw new Error(`rhino-local: empty identifier`);
  }
  return first.toUpperCase() + ident.slice(1);
}

export function tsParamName(name: string): string {
  const renamed = TS_PARAM_RENAMES[name] ?? name;
  return assertIdent(renamed, "parameter name");
}

export function tsType(jsType: string): string {
  switch (jsType) {
    case "string":
      return "string";
    case "number":
      return "number";
    case "boolean":
      return "boolean";
    case "array":
      return "unknown[]";
    case "object":
      return "object";
    case "void":
      return "void";
    case "unknown":
      return "unknown";
    default:
      throw new Error(`rhino-local: no TS mapping for javaScriptType ${jsType}`);
  }
}
