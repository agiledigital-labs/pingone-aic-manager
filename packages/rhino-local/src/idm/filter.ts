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

export function matchesFilter(filter: Filter, object: JsonValue): boolean {
  if (filter.op === "literal") return filter.value;
  if (filter.op === "and") return matchesFilter(filter.left, object) && matchesFilter(filter.right, object);
  if (filter.op === "or") return matchesFilter(filter.left, object) || matchesFilter(filter.right, object);
  if (filter.op === "!") return !matchesFilter(filter.child, object);
  if (!("field" in filter)) return false;
  const value = fieldValue(object, filter.field);
  if (filter.op === "pr") return value !== undefined && value !== null;
  const compare = (candidate: JsonValue): boolean => {
    if (candidate === null || typeof candidate === "object") return false;
    const expected = filter.value;
    switch (filter.op) {
      case "eq": return typeof candidate === "number" ? candidate === Number(expected) : candidate === expected;
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
