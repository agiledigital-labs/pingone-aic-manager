/** CREST recursive-descent parser. Grammar stays separate from repository selection. */
import type { JsonValue } from "../protocol.ts";
import { IdmError } from "./errors.ts";

export type Operator = "eq" | "co" | "sw" | "gt" | "ge" | "lt" | "le";
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
 * How a string comparison treats letter case and edge spaces, per field and operator. `fold` means
 * the rule applies (ASCII lower-casing; trimming U+0020 at both ends), `exact` that it does not, and
 * `unknown` that nobody measured it. The repository knows which fields were measured; the grammar
 * does not.
 */
export type StringRule = "fold" | "exact" | "unknown";
export type Dimension = "case" | "space";
export interface StringPolicy {
  rules(field: string, op: Operator): Record<Dimension, StringRule>;
  /** A `fold` rule changed a leaf's answer; the repository labels it. */
  used(field: string, op: Operator, dimension: Dimension): void;
  /** Every string ordering leaf, so ordering never goes unlabelled. */
  ordered(field: string, op: Operator, ascii: boolean): void;
  /** The record's whole answer turns on an unmeasured rule; must refuse. */
  unknown(field: string, op: Operator, dimension: Dimension): never;
}
type Tri = boolean | "unknown";
type Transform = (text: string) => string;
const same: Transform = (text) => text;
const asciiLower: Transform = (text) => text.replace(/[A-Z]/g, (c) => c.toLowerCase());
const asciiUpper: Transform = (text) => text.replace(/[a-z]/g, (c) => c.toUpperCase());
const fullLower: Transform = (text) => text.toLowerCase();
const trim: Transform = (text) => text.replace(/^ +| +$/g, "");
const collapse: Transform = (text) => trim(text).replace(/ {2,}/g, " ");
// The applied transform first; the rest are readings of the same rule that nothing measured (a
// non-ASCII fold, the fold direction for ordering, whether interior runs of spaces collapse).
const variants: Record<Dimension, Record<StringRule, Transform[]>> = {
  case: { exact: [same], fold: [asciiLower, fullLower, asciiUpper], unknown: [same, asciiLower, fullLower, asciiUpper] },
  space: { exact: [same], fold: [trim, collapse], unknown: [same, trim, collapse] },
};

/**
 * Kleene evaluation per record: a leaf whose answer turns on an unmeasured rule is `unknown`, and
 * only a record whose whole answer is `unknown` refuses, so `and`/`or` operand order cannot matter.
 */
export function matchesFilter(filter: Filter, object: JsonValue, policy?: StringPolicy): boolean {
  let blame: [string, Operator, Dimension] | undefined;
  const result = evaluate(filter, object, policy, (field, op, dimension) => { blame ??= [field, op, dimension]; });
  if (result !== "unknown") return result;
  if (!policy || !blame) throw new Error("unknown filter answer without a policy");
  return policy.unknown(...blame);
}

function evaluate(filter: Filter, object: JsonValue, policy: StringPolicy | undefined, blame: (field: string, op: Operator, dimension: Dimension) => void): Tri {
  if (filter.op === "literal") return filter.value;
  if (filter.op === "and") {
    const left = evaluate(filter.left, object, policy, blame);
    if (left === false) return false;
    const right = evaluate(filter.right, object, policy, blame);
    return right === false ? false : left === true && right === true ? true : "unknown";
  }
  if (filter.op === "or") {
    const left = evaluate(filter.left, object, policy, blame);
    if (left === true) return true;
    const right = evaluate(filter.right, object, policy, blame);
    return right === true ? true : left === false && right === false ? false : "unknown";
  }
  if (filter.op === "!") {
    const child = evaluate(filter.child, object, policy, blame);
    return child === "unknown" ? child : !child;
  }
  if (!("field" in filter)) return false;
  const value = fieldValue(object, filter.field);
  if (filter.op === "pr") return value !== undefined && value !== null;
  const { field, op } = filter;
  const strings = (candidate: string, expected: string): Tri => {
    const test = (a: string, b: string) => {
      switch (op) {
        case "eq": return a === b;
        case "co": return a.includes(b);
        case "sw": return a.startsWith(b);
        case "gt": return a > b;
        case "ge": return a >= b;
        case "lt": return a < b;
        case "le": return a <= b;
      }
    };
    const ordering = op === "gt" || op === "ge" || op === "lt" || op === "le";
    if (ordering) policy?.ordered(field, op, /^[\x20-\x7e]*$/.test(candidate + expected));
    if (!policy) return test(candidate, expected);
    const rules = policy.rules(field, op);
    const run = (c: Transform, s: Transform) => test(c(s(candidate)), c(s(expected)));
    const cases = variants.case[rules.case], spaces = variants.space[rules.space];
    const answer = run(cases[0]!, spaces[0]!);
    for (const c of cases) for (const s of spaces) {
      if (run(c, s) === answer) continue;
      blame(field, op, run(c, spaces[0]!) !== answer ? "case" : "space");
      return "unknown";
    }
    if (answer !== test(candidate, expected)) {
      const caseAlone = run(cases[0]!, same) !== test(candidate, expected), spaceAlone = run(same, spaces[0]!) !== test(candidate, expected);
      if (caseAlone || !spaceAlone) policy.used(field, op, "case");
      if (spaceAlone || !caseAlone) policy.used(field, op, "space");
    }
    return answer;
  };
  const compare = (candidate: JsonValue): Tri => {
    if (candidate === null || typeof candidate === "object") return false;
    const expected = filter.value;
    // The measured DS filter accepts escaped quotes but never matches them.
    if (op === "eq" && typeof expected === "string" && expected.includes('"')) return false;
    if (typeof candidate === "string" && typeof expected === "string") return strings(candidate, expected);
    switch (op) {
      case "eq": return typeof candidate === "number" && typeof expected !== "boolean" ? candidate === Number(expected) : candidate === expected;
      case "co": return typeof candidate === "string" && candidate.includes(String(expected));
      case "sw": return typeof candidate === "string" && candidate.startsWith(String(expected));
      case "gt": return candidate > expected;
      case "ge": return candidate >= expected;
      case "lt": return candidate < expected;
      case "le": return candidate <= expected;
    }
  };
  if (value === undefined) return false;
  if (!Array.isArray(value)) return compare(value);
  const each = value.map(compare);
  return each.includes(true) ? true : each.includes("unknown") ? "unknown" : false;
}
