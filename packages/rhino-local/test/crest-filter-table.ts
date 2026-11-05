import type { JsonObject } from "../src/case/types.ts";

/**
 * One `_queryFilter` table for both lanes (E19 fix 2, P2-1). The grammar is
 * shared (src/bindings/rhino/crest-filter.cjs), and this table is run through
 * the grammar itself, the script tester's openidm.query (node:vm and the JVM)
 * and the IDM emulator (REST and a script's host call), so neither lane can
 * drift from it. Invented records only.
 *
 * `parse` rows are AIC's catchable parse error: measured (docs/api/10, 12) or
 * rejected by any grammar CREST could have. `fault` rows are the
 * `openidm-query-filter` harness fault: the harness cannot say what AIC does.
 */
export const records: Array<JsonObject & { _id: string }> = [
  { _id: "a", text: "alpha", count: 1, flag: true, nested: { "a/b": "match" }, list: ["Red", "x"], spaced: "al  pha", word: "al\u00adpha", other: "\u0130stanbul" },
  { _id: "b", text: "beta", count: 2, flag: false, other: 'say "hi"', spaced: "ga\tma", label: "", list: [], word: "STRA\u1e9eE" },
  { _id: "c", text: "alpine", count: 1000, spaced: "be\u00a0ta", label: "1000", nulls: [null, "y"], word: "jose\u0301", nested: {} },
];

/** What a lane answers: matching ids, AIC's parse error, or the fault. */
export type Answer = string[] | "parse" | "fault";
/**
 * The IDM emulator's own refusal when a record's answer turns on something
 * unmeasured: a `missing` with this cause, which since round 5 carries the
 * `openidm-query-filter` id, so the tally groups it with the AM lane's fault.
 * Only the emulator lanes give it.
 */
export type Missing = { missing: string };

export interface FilterRow {
  filter: unknown;
  /** The shared grammar's own verdict. */
  grammar: "ast" | "parse" | "unsupported";
  /** The script tester (AM lane). */
  am: Answer;
  /** The IDM emulator for a script's host call (journey lane). */
  script: Answer | Missing;
  /** The IDM emulator over REST, where `ne`'s 400 text is measured. */
  rest: Answer | Missing | "ne";
  why: string;
}

const same = (filter: unknown, grammar: FilterRow["grammar"], answer: Answer, why: string): FilterRow =>
  ({ filter, grammar, am: answer, script: answer, rest: answer, why });

const causes = {
  case: "match turns on letter case",
  space: "match turns on leading, trailing or repeated spaces or non-U+0020 whitespace",
  unicode: "match turns on Unicode normalisation or characters RFC 4518 maps to nothing",
  type: "match compares different JSON types, or an object or null element, which nothing measured",
  presence: "answer for an empty string, array or object is unmeasured",
  rule: "field's DS matching rule is not caseIgnoreMatch, which nothing measured",
  path: "pointer reads through an array, measured only as a bare eq reached through and alone",
} as const;
/** The emulator's record-level refusal, with the fault id it carries. */
const refusal = (op: string, why: keyof typeof causes, field: string): Missing =>
  ({ missing: `openidm-query-filter: query ${op} whose ${causes[why]}: widget ${field}` });
/** A row whose answer for some record turns on something unmeasured: the AM fault, the emulator's missing. */
const undecided = (filter: string, why: keyof typeof causes, field: string, note: string, op = "eq"): FilterRow => {
  const missing = refusal(op, why, field);
  return { filter, grammar: "ast", am: "fault", script: missing, rest: missing, why: note };
};

export const filterTable: FilterRow[] = [
  same("true", "ast", ["a", "b", "c"], "literal, measured"),
  same("false", "ast", [], "literal, measured"),
  same('text eq "alpha"', "ast", ["a"], "eq"),
  same('/text eq "alpha"', "ast", ["a"], "leading-slash pointer"),
  same('text co "ph"', "ast", ["a"], "co"),
  same('text sw "al"', "ast", ["a", "c"], "sw"),
  same("flag pr", "ast", ["a", "b"], "pr"),
  same('!(text eq "beta")', "ast", ["a", "c"], "symbolic negation, measured"),
  same('text eq "beta" or count eq 1 and flag eq true', "ast", ["a", "b"], "and binds tighter than or"),
  same('/nested/a~1b eq "match"', "ast", ["a"], "RFC 6901 escape (reviewer: /a~1b)"),
  same('/a~1b eq "x"', "ast", [], "reviewer repro: pointer escape, unknown field"),
  same('userName.first eq "x"', "ast", [], "reviewer repro: dotted name is one key; unknown field is 0 rows, measured"),
  same('/a:b eq "x"', "ast", [], "reviewer repro: any non-space pointer"),
  same('nomé eq "x"', "ast", [], "reviewer repro: non-ASCII pointer"),
  // Round 3 wrongly had `count eq 1e3` here as certainly valid; CREST 27.1.1
  // rejects it (see the fault rows). A `.` makes it a double there.
  // Round 5 P2-2: it parses, but a decimal against a number field is unknown (see below).
  undecided("count eq 1.5e3", "type", "count", "exponent with a `.`: CREST 27.1.1 parses it as a double; against a number, unmeasured"),
  same("count eq 9007199254740991", "ast", [], "2^53 - 1: the largest integer a JS number holds exactly"),
  same("count eq 2", "ast", ["b"], "integer"),
  same('text eq "a\\"b"', "ast", [], "escaped quote parses, measured"),
  // Round-3 P2-2: record b really holds a quote, so only the measured
  // never-matches rule (docs/api/10) gives [] here; plain equality gives ["b"].
  same('other eq "say \\"hi\\""', "ast", [], "an escaped quote never matches, measured, even when the record holds it"),
  same('other sw "say"', "ast", ["b"], "control: the quoted record is reachable"),
  // AIC's parse error.
  same("name eq", "parse", "parse", "missing value, script text measured"),
  same("not (text pr)", "parse", "parse", "word-form not, measured"),
  same("text eq null", "parse", "parse", "eq null, measured"),
  same("text in [1]", "parse", "parse", "in with an array, measured"),
  same("text eq [1]", "parse", "parse", "array value, measured"),
  same('text eq "open', "parse", "parse", "unterminated string"),
  same("(text pr", "parse", "parse", "unclosed ("),
  same("text pr)", "parse", "parse", ") with no ("),
  same("()", "parse", "parse", ") where an expression belongs"),
  same("text", "parse", "parse", "input ends where the operator belongs"),
  same("count eq 1 and", "parse", "parse", "input ends where an expression belongs"),
  // The fault: AIC may parse these.
  same("", "unsupported", "fault", "empty filter, unmeasured (P2-3)"),
  same("   ", "unsupported", "fault", "blank filter, unmeasured (P2-3)"),
  same(null, "unsupported", "fault", "null filter, unmeasured (P2-3)"),
  same("count eq +5", "unsupported", "fault", "reviewer repro: not a JSON number"),
  same("count eq 05", "unsupported", "fault", "not a JSON number"),
  same("count eq 1.2.3", "unsupported", "fault", "not a JSON number"),
  same('text eq "x" AND count pr', "unsupported", "fault", "reviewer repro: keyword case unmeasured"),
  same("TRUE", "unsupported", "fault", "keyword case unmeasured"),
  same("text PR", "unsupported", "fault", "keyword case unmeasured"),
  same('text EQ "x"', "ast", "fault", "a word operator CREST reads; what DS does with it is unmeasured"),
  same("text eq 'x'", "unsupported", "fault", "reviewer repro: single quotes unmeasured"),
  same("!text pr", "unsupported", "fault", "! without ( unmeasured"),
  same("text pr trailing", "unsupported", "fault", "trailing text (could be AND)"),
  same("true false", "unsupported", "fault", "trailing text"),
  same('"text" eq "x"', "unsupported", "fault", "quoted pointer unmeasured"),
  same('text[0] eq "x"', "unsupported", "fault", "pointer with [] unmeasured"),
  same('/a~2 eq "x"', "unsupported", "fault", "invalid RFC 6901 escape"),
  same('text eq"x"', "unsupported", "fault", "no space before a string, unmeasured"),
  same('text in "x"', "ast", "fault", "in with a scalar, unmeasured"),
  // Round-3 P2-1: the on-prem CREST 27.1.1 parser rejects each of these
  // (reviewer's QueryFilters.parse probe). That jar is a hypothesis about AIC,
  // not a measurement, so they are the fault, not AIC's catchable parse error.
  same("count eq 1e3", "unsupported", "fault", "exponent with no `.`: CREST 27.1.1 Long.parseLong refuses it"),
  same("count eq 1E3", "unsupported", "fault", "exponent with no `.`"),
  same("count eq 1e-3", "unsupported", "fault", "exponent with no `.`"),
  same("count eq 99999999999999999999", "unsupported", "fault", "beyond Java long"),
  same("count eq 9223372036854775808", "unsupported", "fault", "Long.MAX_VALUE + 1"),
  same("count eq 9007199254740993", "unsupported", "fault", "2^53 + 1: CREST holds it, a JS number rounds it to 2^53"),
  same('text\teq "x"', "unsupported", "fault", "tab separator: CREST 27.1.1 splits on U+0020 only"),
  same('text eq "x"\tand count pr', "unsupported", "fault", "tab separator"),
  same("text pr\n", "unsupported", "fault", "trailing newline"),
  same("text pr\n and count pr", "unsupported", "fault", "newline separator"),
  same("text pr and!(count pr)", "unsupported", "fault", "! glued to and"),
  same("text pr or!(count pr)", "unsupported", "fault", "! glued to or"),
  // Lanes differ only where the harness does: ordering is evaluated by the
  // IDM emulator only; ne's text is measured over REST only.
  // Round-3 P3-1: AIC folds case and spaces for some fields (measured on
  // alpha_user) and nothing measured the rest. A record whose answer turns on
  // that is refused in every lane: the AM lane's fault, the emulator's own
  // missing. Kleene logic: a false operand decides an `and` without it.
  {
    filter: 'text eq "ALPHA"', grammar: "ast", am: "fault",
    script: refusal("eq", "case", "text"),
    rest: refusal("eq", "case", "text"),
    why: "letter case decides record a; unmeasured for this type",
  },
  {
    filter: 'text eq "alpha "', grammar: "ast", am: "fault",
    script: refusal("eq", "space", "text"),
    rest: refusal("eq", "space", "text"),
    why: "a trailing space decides record a; unmeasured",
  },
  same('text eq "ALPHA" and count eq 2', "ast", [], "unknown and false is false: no record is undecided"),
  {
    filter: 'text sw "AL" or count eq 1000', grammar: "ast", am: "fault",
    script: refusal("sw", "case", "text"),
    rest: refusal("sw", "case", "text"),
    why: "unknown or false is unknown for record a",
  },
  same('text eq "ALPHA" or count eq 1', "ast", ["a"], "unknown or true is true for record a"),
  // Round-4 P2-1: the three mechanisms no row pinned. Each row is a false pass
  // under the plausible wrong version: a two-valued `!`, an array whose
  // undecided element counts as false, and readings without the interior
  // collapse or the non-U+0020 whitespace.
  undecided('!(text eq "ALPHA")', "case", "text", "Kleene !: not unknown is unknown, not true"),
  undecided('list eq "red"', "case", "list", "array: a case-only element match is unknown, not false"),
  same('list eq "Red"', "ast", ["a"], "control: an exact element match decides the array"),
  undecided('spaced eq "al pha"', "space", "spaced", "a run of interior spaces: whether AIC collapses it is unmeasured"),
  undecided('spaced eq "be ta"', "space", "spaced", "U+00A0 inside the value: whether AIC reads it as a space is unmeasured"),
  undecided('spaced eq "ga ma"', "space", "spaced", "a tab inside the value: whether AIC reads it as a space is unmeasured"),
  // Round-4 P3-3: RFC 4518, which DS's caseIgnoreMatch follows, maps a soft
  // hyphen to nothing and applies NFKC; whether AIC does is unmeasured.
  undecided('word eq "alpha"', "unicode", "word", "a soft hyphen: RFC 4518 maps it to nothing, unmeasured on AIC"),
  undecided('word eq "jos\\u00e9"', "unicode", "word", "NFC literal, NFD record: RFC 4518 normalises with NFKC, unmeasured on AIC"),
  same('word eq "beta"', "ast", [], "control: differs under every reading, so decided"),
  // Round-4 P2-3: one type rule for every lane, which these inputs once split
  // (the AM lane matched the first five through String(); the IDM lanes
  // matched the next three through Number()). Same JSON type is decided; a
  // number field against a canonical quoted integer is the one measured
  // mismatch (docs/api/10, "A number matches quoted or unquoted"); every other
  // mismatch, and an object or null element, is unknown.
  undecided('flag eq "true"', "type", "flag", "boolean field, quoted literal: unmeasured"),
  undecided('count sw "10"', "type", "count", "sw on a number field: unmeasured", "sw"),
  undecided('count co "00"', "type", "count", "co on a number field: unmeasured", "co"),
  undecided("label eq 1000", "type", "label", "string field, number literal: unmeasured"),
  undecided('nested co "Object"', "type", "nested", "object field: String() would match [object Object]", "co"),
  undecided('nulls eq ""', "type", "nulls", "a null array element: String() would read it as an empty string"),
  undecided('count eq "1000.0"', "type", "count", "non-canonical quoted number: Number() would round it to 1000"),
  undecided('count eq "01000"', "type", "count", "non-canonical quoted integer"),
  undecided('count eq " 1000"', "type", "count", "quoted integer with a space"),
  // Round-4 P2-2: CREST holds the long; Number() rounds it onto 2^53, a false pass.
  undecided('count eq "9007199254740993"', "type", "count", "quoted integer beyond 2^53 - 1"),
  undecided('count eq "9007199254740992"', "type", "count", "2^53 is spelt canonically, but a fixture value past 2^53 - 1 may already be rounded onto it"),
  // Round-5 P2-1: measured on alpha_user frIndexedInteger1 (an LDAP integer
  // attribute) only. A generic type is JSON in DS, and the script tester knows
  // no types, so here it is unknown in every lane (alpha_user: test/idm/filter.test.ts).
  undecided('count eq "1000"', "type", "count", "quoted integer on a generic type: measured on alpha_user only"),
  same('flag eq "true" and count eq 5', "ast", [], "unknown and false is false: no record is undecided"),
  { filter: 'count gt "5"', grammar: "ast", am: "fault", script: refusal("gt", "type", "count"),
    rest: refusal("gt", "type", "count"), why: "ordering a number field against a quoted literal: unmeasured" },
  // Round-4 P3-1: whether DS stores an empty string or array as present is unmeasured.
  undecided("label pr", "presence", "label", "pr on an empty string", "pr"),
  undecided("list pr", "presence", "list", "pr on an empty array", "pr"),
  same('nulls pr and text eq "alpine"', "ast", ["c"], "control: a non-empty array is present"),
  // The emulator materialises an absent array property as [] (its read
  // shape). Since round 5 (P3-6) it remembers which [] it injected and a query
  // reads those as absent, as DS reads an unset multi-valued attribute; an
  // explicitly written [] (record b's `list`) stays unknown.
  same("nulls pr", "ast", ["c"], "the emulator's injected [] reads as absent, as the fixture does in the AM lane"),
  same("!(nulls pr)", "ast", ["a", "b"], "control: negation of an injected-absent array"),
  // Round-4 P3-2: the escaped-quote rule decides only a bare positive eq
  // reached through `and` alone, where "the leaf is false" and "the query
  // matches nothing" agree. Elsewhere they differ, so the filter is refused.
  same('other eq "say \\"hi\\"" and flag eq false', "ast", [], "escaped quote under and: both readings give no rows"),
  same('!(other eq "say \\"hi\\"")', "ast", "fault", "escaped quote under !: unmeasured"),
  same('text eq "beta" or other eq "say \\"hi\\""', "ast", "fault", "escaped quote under or: unmeasured"),
  same('other co "\\""', "ast", "fault", "escaped quote in co: unmeasured"),
  same('other sw "say \\""', "ast", "fault", "escaped quote in sw: unmeasured"),
  // Round-5 P2-2: a decimal literal keeps its spelling. Against a number
  // field it is unknown: `2.0` may reach an LDAP integer attribute as text
  // integer syntax refuses (filter Undefined), or a JSON number compares it.
  undecided("count eq 2.0", "type", "count", "decimal literal equal to an integer: numerically 2, unmeasured"),
  undecided("count gt 1.5", "type", "count", "decimal literal under ordering: unmeasured", "gt"),
  undecided("!(count eq 2.5)", "type", "count", "decimal under !: !Undefined is Undefined on DS, not true"),
  same("count eq 2", "ast", ["b"], "control: an integer literal is decided"),
  // Round-5 P3-1: per-char lower-casing (U+0130 to i) and a full fold (U+1E9E to ss).
  undecided('other eq "istanbul"', "case", "other", "U+0130: Java's per-char toLowerCase gives i, String toLowerCase i + U+0307"),
  undecided('word eq "strasse"', "case", "word", "U+1E9E: only lower-upper-lower folds it to ss"),
  same('other eq "ankara"', "ast", [], "control: differs under every reading"),
  // Round-5 P3-3: `{}` is the object form of the empty-value question.
  undecided("nested pr", "presence", "nested", "pr on {}", "pr"),
  // Round-5 P3-4: a pointer through an array. Only the bare positive eq form
  // is measured (doc 10: no rows); elsewhere "leaf false" and "query matches
  // nothing" differ. An index the array holds was never measured.
  same('list/ref eq "r1"', "ast", [], "through an array, bare positive eq: no rows, measured"),
  undecided('!(list/ref eq "r1")', "path", "list/ref", "through an array under !: unmeasured"),
  undecided('text eq "beta" or list/ref eq "r1"', "path", "list/ref", "through an array under or: unmeasured"),
  undecided('list/0 eq "Red"', "path", "list/0", "an index into an array: unmeasured"),
  { filter: "count gt 1", grammar: "ast", am: "fault", script: ["b", "c"], rest: ["b", "c"], why: "ordering: not evaluated in the AM lane" },
  { filter: 'text ne "x"', grammar: "ast", am: "fault", script: "fault", rest: "ne", why: "ne: REST 400 measured, script text unmeasured" },
];
