/** The IDM emulator's view of the shared CREST grammar and evaluator (src/bindings/rhino/crest-filter.cjs). */
import { createRequire } from "node:module";
import type { JsonValue } from "../protocol.ts";
import { bindingsCrestFilterPath } from "../paths.ts";

export type Operator = "eq" | "co" | "sw" | "gt" | "ge" | "lt" | "le";
type Ast = { kind: "true" | "false" } | { kind: "and" | "or"; left: Ast; right: Ast } | { kind: "not"; inner: Ast } |
  { kind: "pr"; field: string; positive?: boolean } |
  { kind: "cmp"; field: string; op: string; value: string | number | boolean; spelling?: string; positive?: boolean };
type Prepared = { ast: Ast; error?: undefined } | { error: "parse" | "unsupported" | "ne"; reason: string };
/** A string reading: one way AIC might treat case, spaces or Unicode before comparing. */
type Transform = (text: string) => string;
/** Which unmeasured thing left a leaf undecided. */
export type Dimension = "case" | "space" | "unicode" | "type" | "presence" | "rule" | "path";
type Blame = { field: string; op: string; dimension: Dimension };
/** `true`, `false`, or the leaf that left the answer undecided. */
type Tri = boolean | { unknown: Blame };
type Strings = (field: string, op: string, candidate: string, expected: string) => Tri;
/** What a lane supplies to the shared evaluator (crest-filter.cjs `evaluate`). */
interface Lane {
  strings: Strings;
  quotedInteger(field: string): boolean;
  scalar(field: string, op: string, type: "number" | "boolean"): void;
  /** The field's matching rule (crest-filter.cjs `fieldRule`, or `json` for a JSON-stored property). */
  rule(field: string): FieldRule | null;
  /** This lane applies the measured field rules (the script tester's does not). */
  decides: boolean;
  label(provenance: "measured" | "assumed", behavior: string, source: string): void;
}
/** A field whose DS matching rule is not a string reading (crest-filter.cjs `fieldRule`). */
export type FieldRule = "phone" | "instant" | "integer" | "dsInteger" | "dsRefused" | "emptyAbsent" | "json" | "jsonOwn";
/** A literal AIC refuses with a 400 before reading any record. */
export type Illegal = { field: string; value: string; op: string };

const grammar = createRequire(import.meta.url)(bindingsCrestFilterPath) as {
  pointerParts(path: string): string[];
  prepare(input: unknown, evaluated: readonly string[]): Prepared;
  evaluate(ast: Ast, record: JsonValue, lane: Lane): Tri;
  fieldRule(type: string, field: string): FieldRule | null;
  assertions(ast: Ast, lane: Lane): { unknown: Blame } | { illegal: Illegal } | null;
  readField(record: JsonValue, path: string): JsonValue | undefined;
  readStrings(candidate: string, op: string, expected: string, cases: Transform[], spaces: Transform[]):
    { answer: boolean; dimension: Dimension | null };
  compareText(left: string, op: string, right: string): boolean;
  transforms: Record<"identity" | "asciiLower" | "asciiUpper" | "fullLower" | "foldLower" | "charLower" | "fullFold" |
    "trim" | "collapse" | "wideTrim" | "wideCollapse", Transform>;
};

export const pointerParts = (path: string): string[] => grammar.pointerParts(path);

export const fieldValue = (value: JsonValue, path: string): JsonValue | undefined => grammar.readField(value, path);

/** A user type's field whose DS matching rule no string reading describes (round 5; measured 2026-11-06). */
export const fieldRule = (type: string, field: string): FieldRule | null => grammar.fieldRule(type, field);

const evaluated: readonly Operator[] = ["eq", "co", "sw", "gt", "ge", "lt", "le"];

/** A filter the shared grammar read and this emulator evaluates. */
export type Filter = Ast;

/**
 * The shared grammar's verdict: a filter this emulator evaluates; `parse`, AIC's measured (or
 * structurally certain) parse error; `ne`, a word operator AIC parses and then refuses; or
 * `unsupported`, anything the emulator cannot decide, which the caller must refuse as missing.
 */
export type Classified = { filter: Filter } | { error: "parse" } | { error: "ne" } | { error: "unsupported"; reason: string };

export function classifyFilter(input: unknown): Classified {
  const prepared = grammar.prepare(input, evaluated);
  if (prepared.error === "parse" || prepared.error === "ne") return { error: prepared.error };
  if (prepared.error) return { error: "unsupported", reason: prepared.reason };
  return { filter: prepared.ast };
}

/**
 * How a string comparison treats letter case and edge spaces, per field and operator. `fold` means
 * the rule applies (ASCII lower-casing; trimming U+0020 at both ends — other readings are checked
 * as variants), `exact` that it does not, and `unknown` that nobody measured it. The repository
 * knows which fields were measured; the grammar does not.
 */
export type StringRule = "fold" | "exact" | "unknown";
/** The two dimensions a per-field policy speaks for; the rest are never measured. */
export type RuleDimension = "case" | "space";
export interface StringPolicy {
  rules(field: string, op: Operator): Record<RuleDimension, StringRule>;
  /** A `fold` rule changed a leaf's answer; the repository labels it. */
  used(field: string, op: Operator, dimension: RuleDimension): void;
  /** Every string ordering leaf, so ordering never goes unlabelled. */
  ordered(field: string, op: Operator, ascii: boolean): void;
  /** The measured `eq "7"` against a number (doc 10) holds for this field; labels it if so. */
  quotedInteger(field: string): boolean;
  /** A same-type number or boolean leaf was decided; the repository labels it. */
  scalar(field: string, op: Operator, type: "number" | "boolean"): void;
  /** The field's matching rule, where it is not a string reading. */
  rule(field: string): FieldRule | null;
  /** A measured field rule decided a leaf; the repository labels it. */
  label(provenance: "measured" | "assumed", behavior: string, source: string): void;
  /** The record's whole answer turns on something unmeasured; must refuse. */
  unknown(field: string, op: Operator | "pr", dimension: Dimension): never;
}

const t = grammar.transforms;
// The applied transform first; the rest are readings of the same rule that nothing measured (a
// non-ASCII fold, the fold direction for ordering, whether interior runs of spaces collapse,
// whether whitespace other than U+0020 counts as a space). The shared evaluator adds the Unicode
// readings (NFKC, RFC 4518 map-to-nothing) on top, for every field.
const variants: Record<RuleDimension, Record<StringRule, Transform[]>> = {
  case: { exact: [t.identity], fold: [t.asciiLower, t.fullLower, t.asciiUpper, t.foldLower, t.charLower, t.fullFold],
    unknown: [t.identity, t.asciiLower, t.fullLower, t.asciiUpper, t.foldLower, t.charLower, t.fullFold] },
  space: { exact: [t.identity], fold: [t.trim, t.collapse, t.wideTrim, t.wideCollapse], unknown: [t.identity, t.trim, t.collapse, t.wideTrim, t.wideCollapse] },
};

const ORDERING: readonly string[] = ["gt", "ge", "lt", "le"];

function policyLane(policy: StringPolicy): Lane {
  return {
    strings: policyStrings(policy),
    quotedInteger: (field) => policy.quotedInteger(field),
    scalar: (field, op, type) => policy.scalar(field, op as Operator, type),
    rule: (field) => policy.rule(field),
    decides: true,
    label: (provenance, behavior, source) => policy.label(provenance, behavior, source),
  };
}

function policyStrings(policy: StringPolicy): Strings {
  return (field, operator, candidate, expected) => {
    const op = operator as Operator;
    if (ORDERING.includes(op)) policy.ordered(field, op, /^[\x20-\x7e]*$/.test(candidate + expected));
    const rules = policy.rules(field, op);
    const cases = variants.case[rules.case], spaces = variants.space[rules.space];
    const read = grammar.readStrings(candidate, op, expected, cases, spaces);
    if (read.dimension !== null) return { unknown: { field, op, dimension: read.dimension } };
    const exact = grammar.compareText(candidate, op, expected);
    if (read.answer !== exact) {
      const run = (c: Transform, s: Transform) => grammar.compareText(c(s(candidate)), op, c(s(expected)));
      const caseAlone = run(cases[0]!, t.identity) !== exact, spaceAlone = run(t.identity, spaces[0]!) !== exact;
      if (caseAlone || !spaceAlone) policy.used(field, op, "case");
      if (spaceAlone || !caseAlone) policy.used(field, op, "space");
    }
    return read.answer;
  };
}

/**
 * The leaves AIC judges before reading a record: a literal a measured field rule refuses (AIC's
 * 400, returned for the caller to throw), or one this emulator cannot judge (refused through the
 * policy, even when the collection is empty). `null` when the filter goes on to the records.
 */
export function illegalAssertion(filter: Filter, policy: StringPolicy): Illegal | null {
  const verdict = grammar.assertions(filter, policyLane(policy));
  if (verdict === null) return null;
  if ("illegal" in verdict) return verdict.illegal;
  const { field, op, dimension } = verdict.unknown;
  return policy.unknown(field, op as Operator | "pr", dimension);
}

/**
 * Kleene evaluation per record (shared with the script tester): a leaf whose answer turns on
 * something unmeasured is `unknown`, and only a record whose whole answer is `unknown` refuses, so
 * `and`/`or` operand order cannot matter.
 */
export function matchesFilter(filter: Filter, object: JsonValue, policy: StringPolicy): boolean {
  const result = grammar.evaluate(filter, object, policyLane(policy));
  if (typeof result === "boolean") return result;
  const { field, op, dimension } = result.unknown;
  return policy.unknown(field, op as Operator | "pr", dimension);
}
