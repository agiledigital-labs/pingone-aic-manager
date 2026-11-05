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
export const records = [
  { _id: "a", text: "alpha", count: 1, flag: true, nested: { "a/b": "match" }, list: ["Red", "x"], spaced: "al  pha" },
  { _id: "b", text: "beta", count: 2, flag: false, other: 'say "hi"', spaced: "ga\tma" },
  { _id: "c", text: "alpine", count: 1000, spaced: "be\u00a0ta" },
] as const;

/** What a lane answers: matching ids, AIC's parse error, or the fault. */
export type Answer = string[] | "parse" | "fault";
/**
 * The IDM emulator's own refusal when a record's answer turns on an unmeasured
 * case or space rule: an id-less `missing` with this cause, which fails the run
 * as the fault does. Only the emulator lanes give it.
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
  case: "letter case",
  space: "leading, trailing or repeated spaces or non-U+0020 whitespace",
} as const;
/** A row whose answer for some record turns on an unmeasured rule: the AM fault, the emulator's missing. */
const undecided = (filter: string, why: keyof typeof causes, field: string, note: string, op = "eq"): FilterRow => {
  const missing = { missing: `query ${op} whose match turns on ${causes[why]}: widget ${field}` };
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
  same("count eq 1.5e3", "ast", [], "exponent with a `.`: CREST 27.1.1 parses it as a double"),
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
    script: { missing: "query eq whose match turns on letter case: widget text" },
    rest: { missing: "query eq whose match turns on letter case: widget text" },
    why: "letter case decides record a; unmeasured for this type",
  },
  {
    filter: 'text eq "alpha "', grammar: "ast", am: "fault",
    script: { missing: "query eq whose match turns on leading, trailing or repeated spaces or non-U+0020 whitespace: widget text" },
    rest: { missing: "query eq whose match turns on leading, trailing or repeated spaces or non-U+0020 whitespace: widget text" },
    why: "a trailing space decides record a; unmeasured",
  },
  same('text eq "ALPHA" and count eq 2', "ast", [], "unknown and false is false: no record is undecided"),
  {
    filter: 'text sw "AL" or count eq 1000', grammar: "ast", am: "fault",
    script: { missing: "query sw whose match turns on letter case: widget text" },
    rest: { missing: "query sw whose match turns on letter case: widget text" },
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
  { filter: "count gt 1", grammar: "ast", am: "fault", script: ["b", "c"], rest: ["b", "c"], why: "ordering: not evaluated in the AM lane" },
  { filter: 'text ne "x"', grammar: "ast", am: "fault", script: "fault", rest: "ne", why: "ne: REST 400 measured, script text unmeasured" },
];
