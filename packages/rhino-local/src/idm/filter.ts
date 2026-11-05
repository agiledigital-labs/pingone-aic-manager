/** The IDM emulator's view of the shared CREST grammar (src/bindings/rhino/crest-filter.cjs). */
import { createRequire } from "node:module";
import type { JsonValue } from "../protocol.ts";
import { bindingsCrestFilterPath } from "../paths.ts";

export type Operator = "eq" | "co" | "sw" | "gt" | "ge" | "lt" | "le";
type Filter = { op: "literal"; value: boolean } |
  { op: "and" | "or"; left: Filter; right: Filter } |
  { op: "!"; child: Filter } | { op: "pr"; field: string } |
  { op: Operator; field: string; value: string | number | boolean };

type Ast = { kind: "true" | "false" } | { kind: "and" | "or"; left: Ast; right: Ast } | { kind: "not"; inner: Ast } |
  { kind: "pr"; field: string } | { kind: "cmp"; field: string; op: string; value: string | number | boolean };
type Parsed = { ast: Ast; error?: undefined } | { error: "parse" | "unsupported"; reason: string };
const grammar = createRequire(import.meta.url)(bindingsCrestFilterPath) as {
  parse(input: unknown): Parsed;
  pointerParts(path: string): string[];
};

export const pointerParts = (path: string): string[] => grammar.pointerParts(path);

export function fieldValue(value: JsonValue, path: string): JsonValue | undefined {
  let current: JsonValue | undefined = value;
  for (const part of pointerParts(path)) {
    if (current === null || typeof current !== "object" || !Object.hasOwn(current, part)) return undefined;
    current = (current as Record<string, JsonValue>)[part];
  }
  return current;
}

const evaluated: readonly string[] = ["eq", "co", "sw", "gt", "ge", "lt", "le"];

/**
 * The shared grammar's verdict: a filter this emulator evaluates; `parse`, AIC's measured (or
 * structurally certain) parse error; `ne`, a word operator AIC parses and then refuses; or
 * `unsupported`, anything the emulator cannot decide, which the caller must refuse as missing.
 */
export type Classified = { filter: Filter } | { error: "parse" } | { error: "ne" } | { error: "unsupported"; reason: string };

export function classifyFilter(input: unknown): Classified {
  const parsed = grammar.parse(input);
  if (parsed.error === "parse") return { error: "parse" };
  if (parsed.error) return { error: "unsupported", reason: parsed.reason };
  let ne = false;
  let unknown: string | undefined;
  const convert = (node: Ast): Filter => {
    switch (node.kind) {
      case "true": case "false": return { op: "literal", value: node.kind === "true" };
      case "and": case "or": return { op: node.kind, left: convert(node.left), right: convert(node.right) };
      case "not": return { op: "!", child: convert(node.inner) };
      case "pr": return { op: "pr", field: node.field };
      case "cmp":
        if (node.op === "ne") ne = true;
        else if (!evaluated.includes(node.op)) unknown ??= `operator ${node.op} is not evaluated by the emulator`;
        return { op: node.op as Operator, field: node.field, value: node.value };
    }
  };
  const filter = convert(parsed.ast);
  if (unknown) return { error: "unsupported", reason: unknown };
  return ne ? { error: "ne" } : { filter };
}

/**
 * How a string comparison treats letter case and edge spaces, per field and operator. `fold` means
 * the rule applies (ASCII lower-casing; trimming U+0020 at both ends — other readings are checked as variants), `exact` that it does not, and
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
type Blame = [field: string, op: Operator, dimension: Dimension];
/** `true`, `false`, or the leaf that left the answer undecided. */
type Tri = boolean | { unknown: Blame };
type Transform = (text: string) => string;
const same: Transform = (text) => text;
const asciiLower: Transform = (text) => text.replace(/[A-Z]/g, (c) => c.toLowerCase());
const asciiUpper: Transform = (text) => text.replace(/[a-z]/g, (c) => c.toUpperCase());
const fullLower: Transform = (text) => text.toLowerCase();
const trim: Transform = (text) => text.replace(/^ +| +$/g, "");
const collapse: Transform = (text) => trim(text).replace(/ {2,}/g, " ");
// RFC 4518 §2.2 maps these to SPACE before insignificant-space handling; nothing measured whether AIC does.
const toSpace: Transform = (text) => text.replace(/[\t-\r\x85\xa0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]/g, " ");
const wideTrim: Transform = (text) => trim(toSpace(text));
const wideCollapse: Transform = (text) => collapse(toSpace(text));
// Upper then lower maps ß to "ss", as RFC 4518 case folding does; no other variant differs on it.
const foldLower: Transform = (text) => text.toUpperCase().toLowerCase();
// The applied transform first; the rest are readings of the same rule that nothing measured (a
// non-ASCII fold, the fold direction for ordering, whether interior runs of spaces collapse,
// whether whitespace other than U+0020 counts as a space).
const variants: Record<Dimension, Record<StringRule, Transform[]>> = {
  case: { exact: [same], fold: [asciiLower, fullLower, asciiUpper, foldLower], unknown: [same, asciiLower, fullLower, asciiUpper, foldLower] },
  space: { exact: [same], fold: [trim, collapse, wideTrim, wideCollapse], unknown: [same, trim, collapse, wideTrim, wideCollapse] },
};

/**
 * Kleene evaluation per record: a leaf whose answer turns on an unmeasured rule is `unknown`, and
 * only a record whose whole answer is `unknown` refuses, so `and`/`or` operand order cannot matter.
 */
export function matchesFilter(filter: Filter, object: JsonValue, policy?: StringPolicy): boolean {
  const result = evaluate(filter, object, policy);
  if (typeof result === "boolean") return result;
  if (!policy) throw new Error("unknown filter answer without a policy");
  return policy.unknown(...result.unknown);
}

// An undecided result carries its own blame, so a leaf that `and`/`or` made irrelevant is never named.
function evaluate(filter: Filter, object: JsonValue, policy: StringPolicy | undefined): Tri {
  if (filter.op === "literal") return filter.value;
  if (filter.op === "and") {
    const left = evaluate(filter.left, object, policy);
    if (left === false) return false;
    const right = evaluate(filter.right, object, policy);
    return right === false ? false : left === true ? right : left;
  }
  if (filter.op === "or") {
    const left = evaluate(filter.left, object, policy);
    if (left === true) return true;
    const right = evaluate(filter.right, object, policy);
    return right === true ? true : left === false ? right : left;
  }
  if (filter.op === "!") {
    const child = evaluate(filter.child, object, policy);
    return typeof child === "boolean" ? !child : child;
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
      return { unknown: [field, op, run(c, spaces[0]!) !== answer ? "case" : "space"] };
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
  return each.includes(true) ? true : each.find((t) => typeof t !== "boolean") ?? false;
}
