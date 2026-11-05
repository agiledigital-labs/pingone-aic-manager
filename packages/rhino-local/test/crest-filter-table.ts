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
  { _id: "a", text: "alpha", count: 1, flag: true, nested: { "a/b": "match" } },
  { _id: "b", text: "beta", count: 2, flag: false },
  { _id: "c", text: "alpine", count: 1000 },
] as const;

/** What a lane answers: matching ids, AIC's parse error, or the fault. */
export type Answer = string[] | "parse" | "fault";

export interface FilterRow {
  filter: unknown;
  /** The shared grammar's own verdict. */
  grammar: "ast" | "parse" | "unsupported";
  /** The script tester (AM lane). */
  am: Answer;
  /** The IDM emulator for a script's host call (journey lane). */
  script: Answer;
  /** The IDM emulator over REST, where `ne`'s 400 text is measured. */
  rest: Answer | "ne";
  why: string;
}

const same = (filter: unknown, grammar: FilterRow["grammar"], answer: Answer, why: string): FilterRow =>
  ({ filter, grammar, am: answer, script: answer, rest: answer, why });

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
  same("count eq 1e3", "ast", ["c"], "reviewer repro: JSON exponent"),
  same("count eq 2", "ast", ["b"], "integer"),
  same('text eq "a\\"b"', "ast", [], "escaped quote parses, measured"),
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
  // Lanes differ only where the harness does: ordering is evaluated by the
  // IDM emulator only; ne's text is measured over REST only.
  { filter: "count gt 1", grammar: "ast", am: "fault", script: ["b", "c"], rest: ["b", "c"], why: "ordering: not evaluated in the AM lane" },
  { filter: 'text ne "x"', grammar: "ast", am: "fault", script: "fault", rest: "ne", why: "ne: REST 400 measured, script text unmeasured" },
];
