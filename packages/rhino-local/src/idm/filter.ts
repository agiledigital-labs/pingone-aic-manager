/** CREST recursive-descent parser. Grammar stays separate from repository selection. */
import type { JsonValue } from "../protocol.ts";
import { IdmError } from "./errors.ts";

type Operator = "eq" | "co" | "sw" | "gt" | "ge" | "lt" | "le";
type Filter = { op: "literal"; value: boolean } |
  { op: "and" | "or"; left: Filter; right: Filter } |
  { op: "!"; child: Filter } | { op: "pr"; field: string } |
  { op: Operator; field: string; value: string | number | boolean };
type Token = { text: string; literal?: string | number | boolean };
const invalid = () => new IdmError(400, "A value could not be parsed as a valid query filter");

export function pointerParts(path: string): string[] {
  return path.replace(/^\//, "").split("/").map((part) => part.replace(/~1/g, "/").replace(/~0/g, "~"));
}

export function fieldValue(value: JsonValue, path: string): JsonValue | undefined {
  let current: JsonValue | undefined = value;
  for (const part of pointerParts(path)) {
    if (current === null || typeof current !== "object" || !Object.hasOwn(current, part)) return undefined;
    current = (current as Record<string, JsonValue>)[part];
  }
  return current;
}

export function parseFilter(input: string): Filter {
  const tokens: Token[] = [];
  const re = /\s*("(?:[^"\\]|\\.)*"|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?|[!()]|[^\s!()"]+)/gy;
  let offset = 0;
  while (offset < input.length) {
    if (input.slice(offset).trim() === "") break;
    re.lastIndex = offset;
    const match = re.exec(input);
    if (!match?.[1]) throw invalid();
    const text = match[1];
    offset = re.lastIndex;
    if (text.startsWith('"')) {
      try { tokens.push({ text, literal: JSON.parse(text) as string }); }
      catch { throw invalid(); }
    } else if (/^-?\d/.test(text)) {
      const number = Number(text);
      if (!Number.isFinite(number)) throw invalid();
      tokens.push({ text, literal: number });
    } else if (text === "true" || text === "false") tokens.push({ text, literal: text === "true" });
    else tokens.push({ text });
  }
  let index = 0;
  const take = (text: string) => {
    if (tokens[index]?.text !== text) return false;
    index++;
    return true;
  };
  const primary = (): Filter => {
    if (take("!")) return { op: "!", child: primary() };
    if (take("(")) {
      const result = or();
      if (!take(")")) throw invalid();
      return result;
    }
    const field = tokens[index++];
    if (!field) throw invalid();
    if (typeof field.literal === "boolean") return { op: "literal", value: field.literal };
    if (field.literal !== undefined || !/^(?:\/)?[^\s[\]{}]+$/.test(field.text)) throw invalid();
    const op = tokens[index++]?.text;
    if (op === "pr") return { op, field: field.text };
    if (op === "ne") throw new IdmError(400, "unrecognized or unsupported filter operator 'ne'");
    if (!op || !["eq", "co", "sw", "gt", "ge", "lt", "le"].includes(op)) throw invalid();
    const value = tokens[index++]?.literal;
    if (value === undefined) throw invalid();
    return { op: op as Operator, field: field.text, value };
  };
  const and = (): Filter => {
    let result = primary();
    while (take("and")) result = { op: "and", left: result, right: primary() };
    return result;
  };
  const or = (): Filter => {
    let result = and();
    while (take("or")) result = { op: "or", left: result, right: and() };
    return result;
  };
  const result = or();
  if (index !== tokens.length) throw invalid();
  return result;
}

/**
 * Answers a string `eq`/`co`/`sw` whose result turns on letter case alone (`ascii`: ASCII case
 * folding alone explains it). The repository knows which fields were measured; the grammar does not.
 */
export type CaseOnly = (field: string, op: "eq" | "co" | "sw", ascii: boolean) => boolean;
const asciiLower = (text: string) => text.replace(/[A-Z]/g, (c) => c.toLowerCase());

export function matchesFilter(filter: Filter, object: JsonValue, caseOnly?: CaseOnly): boolean {
  if (filter.op === "literal") return filter.value;
  if (filter.op === "and") return matchesFilter(filter.left, object, caseOnly) && matchesFilter(filter.right, object, caseOnly);
  if (filter.op === "or") return matchesFilter(filter.left, object, caseOnly) || matchesFilter(filter.right, object, caseOnly);
  if (filter.op === "!") return !matchesFilter(filter.child, object, caseOnly);
  if (!("field" in filter)) return false;
  const value = fieldValue(object, filter.field);
  if (filter.op === "pr") return value !== undefined && value !== null;
  const strings = (op: "eq" | "co" | "sw", candidate: string, expected: string): boolean => {
    const test = (a: string, b: string) => op === "eq" ? a === b : op === "co" ? a.includes(b) : a.startsWith(b);
    const exact = test(candidate, expected);
    if (!caseOnly || exact === test(candidate.toLowerCase(), expected.toLowerCase())) return exact;
    return caseOnly(filter.field, op, test(asciiLower(candidate), asciiLower(expected)) !== exact);
  };
  const compare = (candidate: JsonValue): boolean => {
    if (candidate === null || typeof candidate === "object") return false;
    const expected = filter.value;
    if ((filter.op === "eq" || filter.op === "co" || filter.op === "sw") && typeof candidate === "string" &&
        typeof expected === "string" && !(filter.op === "eq" && expected.includes('"'))) {
      return strings(filter.op, candidate, expected);
    }
    switch (filter.op) {
      case "eq":
        // The measured DS filter accepts escaped quotes but never matches them.
        if (typeof expected === "string" && expected.includes('"')) return false;
        return typeof candidate === "number" && typeof expected !== "boolean" ? candidate === Number(expected) : candidate === expected;
      case "co": return typeof candidate === "string" && candidate.includes(String(expected));
      case "sw": return typeof candidate === "string" && candidate.startsWith(String(expected));
      case "gt": return candidate > expected;
      case "ge": return candidate >= expected;
      case "lt": return candidate < expected;
      case "le": return candidate <= expected;
    }
  };
  return value !== undefined && (Array.isArray(value) ? value.some(compare) : compare(value));
}
