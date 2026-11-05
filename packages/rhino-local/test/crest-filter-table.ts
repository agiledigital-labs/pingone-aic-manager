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

export const causes = {
  case: "match turns on letter case",
  space: "match turns on leading, trailing or repeated spaces or non-U+0020 whitespace",
  unicode: "match turns on Unicode normalisation or characters RFC 4518 maps to nothing",
  type: "match compares different JSON types, or an object or null element, which nothing measured",
  presence: "answer for an empty string, array or object is unmeasured",
  rule: "field's DS matching rule is not caseIgnoreMatch (telephone, time or integer), and the 2026-11-06 probe measured neither this field nor this operator and literal form",
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

/**
 * E19 live probe (docs/api/10, 2026-11-06): rows that turn on the collection,
 * because the answer depends on a field's DS matching rule or on how DS stores
 * the type. Records mirror the probe's (`a`, `b`, `c` on alpha_user, §3; `s7`,
 * `s8`, `s75` on a generic type, §4), invented values only. `sink` is generic
 * through the `managed/*` mapping alone, the probe's shape; `linkedsink` has a
 * `genericMapping` entry of its own, which the probe did not reach (decided,
 * labelled assumed), and maps `linked` to an attribute; `pinned` is under
 * `explicitMapping`. The AM lane knows collections by name only, so it has no
 * repo and no JSON rule; its openidm.query returned the REST rows on every
 * alpha_user rule (second probe, 2026-11-06), so it decides those, with its
 * own 400 surface, and evaluates ordering only on the instant fields.
 */
const user = (extra: JsonObject) => ({ userName: "u", ...extra });
export const collectionRecords: Record<string, Array<JsonObject & { _id: string }>> = {
  alpha_user: [
    { _id: "a", ...user({ telephoneNumber: "+614-9999-9999", frIndexedDate1: "2026-01-01T10:00:00+11:00", frUnindexedDate1: "2026-01-01T10:00:00+11:00",
      frIndexedDate2: "2026-01-01T10:00:00+11:00", frIndexedInteger1: 7, frIndexedInteger2: 7, frUnindexedInteger1: 7,
      frIndexedMultivalued1: [], frUnindexedMultivalued1: [], roles: [], frIndexedMultivalued2: [] }) },
    { _id: "b", ...user({ telephoneNumber: "+614-1111-1111", frIndexedDate1: "2026-01-02T00:00:00Z", frUnindexedDate1: "2026-01-02T00:00:00Z",
      frIndexedInteger1: 8, frIndexedMultivalued1: ["x"], frUnindexedMultivalued1: ["x"], roles: ["r"] }) },
    { _id: "c", ...user({}) },
  ],
  bravo_user: [
    { _id: "a", ...user({ telephoneNumber: "+614-9999-9999", frIndexedDate1: "2026-01-01T10:00:00+11:00", frIndexedInteger1: 7, frIndexedMultivalued1: [] }) },
  ],
  sink: [{ _id: "s7", count: 7 }, { _id: "s8", count: 8 }, { _id: "s75", count: 7.5 }],
  linkedsink: [{ _id: "l7", count: 7, linked: 7 }],
  pinned: [{ _id: "p7", count: 7 }],
};
/** The `config/repo.ds` shape the emulator reads storage from (doc 10; invented entries). */
export const repo = {
  resourceMapping: {
    explicitMapping: { "managed/pinned": { objectClasses: ["top", "frPinned"] } },
    genericMapping: { "managed/*": { objectClasses: ["uidObject", "fr-idm-managed-object"] },
      "managed/linkedsink": { objectClasses: ["uidObject", "fr-idm-managed-object"], properties: { linked: { type: "simple", ldapAttribute: "fr-idm-linked" } } } },
  },
};

/** AIC's 400 for a literal the field's DS syntax cannot take (measured over REST, IDM scripts and AM's openidm.query). */
export type Illegal = { illegal: string; field: string };
export const illegalMessage = (value: string, field: string) =>
  `The request cannot be processed because it contained an illegal filter assertion value '${value}' for field '${field}'`;
export interface CollectionRow {
  collection: string;
  filter: string;
  /** The script tester (AM lane): its own 400 surface for `illegal` (docs/api/12). */
  am: Answer | Illegal;
  /** The IDM emulator, over REST and from a script alike (the probe found them identical). */
  idm: string[] | Missing | Illegal;
  why: string;
  /** The collection's records for this row alone, in place of `collectionRecords` (the off-form guard rows). */
  records?: Array<JsonObject & { _id: string }>;
}
/** Every collection's records as `row` sees them. */
export const recordsFor = (row: CollectionRow): typeof collectionRecords =>
  row.records === undefined ? collectionRecords : { ...collectionRecords, [row.collection]: row.records };
const missingOn = (type: string, op: string, why: keyof typeof causes, field: string): Missing =>
  ({ missing: `openidm-query-filter: query ${op} whose ${causes[why]}: ${type} ${field}` });
const alpha = (filter: string, am: CollectionRow["am"], idm: CollectionRow["idm"], why: string): CollectionRow => ({ collection: "alpha_user", filter, am, idm, why });
const on = (collection: string) => (filter: string, am: CollectionRow["am"], idm: CollectionRow["idm"], why: string): CollectionRow => ({ collection, filter, am, idm, why });
const bravo = on("bravo_user"), sink = on("sink"), linkedsink = on("linkedsink"), pinned = on("pinned");
const illegal = (value: string, field: string): Illegal => ({ illegal: value, field });
/**
 * A stored value outside the form a rule decides (review pf-02 P2-1): the rule's guard refuses the
 * record, so every lane must too. One record per row, so the probe-mirroring a/b/c stay as they are.
 */
const offForm = (stored: JsonObject, filter: string, op: string, why: keyof typeof causes, field: string, note: string): CollectionRow =>
  ({ collection: "alpha_user", filter, am: "fault", idm: missingOn("alpha_user", op, why, field), why: note, records: [{ _id: "d", ...user(stored) }] });

export const collectionTable: CollectionRow[] = [
  // §3a telephoneNumberMatch: spaces, hyphens and `+` ignored anywhere, eq/sw/co (both probes;
  // the second ran each row through REST, IDM's and AM's openidm.query alike).
  alpha('telephoneNumber eq "+614-9999-9999"', ["a"], ["a"], "exact control (measured)"),
  alpha('telephoneNumber eq "+614-9999-9998"', [], [], "negative control (measured)"),
  alpha('telephoneNumber eq "+614 9999 9999"', ["a"], ["a"], "spaces for hyphens (measured)"),
  alpha('telephoneNumber eq "+61499999999"', ["a"], ["a"], "no separators (measured)"),
  alpha('telephoneNumber eq "+61 4 9999 9999"', ["a"], ["a"], "spaces anywhere (measured)"),
  alpha('telephoneNumber eq "+6 1-4 99 99-9999"', ["a"], ["a"], "mixed, anywhere (measured)"),
  alpha('telephoneNumber sw "+6149"', ["a"], ["a"], "sw ignores the stored hyphen (measured)"),
  alpha('telephoneNumber co "99 99"', ["a"], ["a"], "co ignores both (measured)"),
  alpha('telephoneNumber eq "61499999999"', ["a"], ["a"], "no + in the literal: + is not significant (measured)"),
  alpha('telephoneNumber eq "614-9999-9999"', ["a"], ["a"], "no + with hyphens (measured)"),
  alpha('telephoneNumber eq "6+1499999999"', ["a"], ["a"], "+ in the middle is ignored too (measured)"),
  alpha('telephoneNumber eq "61499999998"', [], [], "negative control without + (measured)"),
  alpha('telephoneNumber eq "+61499999998"', [], [], "negative control with + (measured)"),
  alpha('telephoneNumber sw "614"', ["a", "b"], ["a", "b"], "sw without +: both stored values start +614 (measured)"),
  alpha('telephoneNumber sw "+614"', ["a", "b"], ["a", "b"], "sw with + (measured)"),
  alpha('telephoneNumber co "+9999"', ["a"], ["a"], "co with a + the stored value lacks there (measured)"),
  alpha('telephoneNumber co "9999"', ["a"], ["a"], "co without + (measured)"),
  alpha("telephoneNumber pr", ["a", "b"], ["a", "b"], "pr control (measured)"),
  alpha('telephoneNumber eq "+"', "fault", missingOn("alpha_user", "eq", "rule", "telephoneNumber"), "no digit: strips to an empty literal, not probed"),
  alpha('telephoneNumber eq "(04) 9999 9999"', "fault", missingOn("alpha_user", "eq", "rule", "telephoneNumber"), "parentheses were not probed"),
  alpha('telephoneNumber gt "+614"', "fault", missingOn("alpha_user", "gt", "rule", "telephoneNumber"), "ordering on a phone was not probed"),
  alpha('telephoneNumber eq "(04) 9999 9999" or userName eq "u"', "fault", missingOn("alpha_user", "eq", "rule", "telephoneNumber"),
    "the leaf is refused before any record, even where or makes it irrelevant"),
  alpha('telephoneNumber eq "+614-9999-9999" or userName eq "u"', ["a", "b", "c"], ["a", "b", "c"], "control: a decided leaf under or"),
  // §3b GeneralizedTime: instants, both families; a non-date literal is the 400.
  alpha('frIndexedDate1 gt "2025-06-01T00:00:00Z"', ["a", "b"], ["a", "b"], "control: both later (measured)"),
  alpha('frIndexedDate1 gt "2026-01-01T00:00:00Z"', ["b"], ["b"], "a's offset instant is earlier though it sorts later (measured)"),
  alpha('frIndexedDate1 lt "2026-01-01T00:00:00Z"', ["a"], ["a"], "measured"),
  alpha('frUnindexedDate1 gt "2026-01-01T00:00:00Z"', ["b"], ["b"], "unindexed family (measured)"),
  alpha('frUnindexedDate1 lt "2026-01-01T00:00:00Z"', ["a"], ["a"], "unindexed family (measured)"),
  alpha('frIndexedDate1 eq "2026-01-01T10:00:00+11:00"', ["a"], ["a"], "as written (measured)"),
  alpha('frIndexedDate1 eq "2025-12-31T23:00:00Z"', ["a"], ["a"], "same instant, other offset (measured)"),
  alpha('frUnindexedDate1 eq "2025-12-31T23:00:00Z"', ["a"], ["a"], "same instant, unindexed (measured)"),
  alpha('frIndexedDate1 eq "2026-01-02T00:00:00Z"', ["b"], ["b"], "control b (measured)"),
  alpha('frIndexedDate1 gt "2026-01-01T09:00:00+11:00"', ["a", "b"], ["a", "b"], "offset literal (measured)"),
  alpha('frIndexedDate1 lt "2026-01-01T09:00:00+11:00"', [], [], "offset literal (measured)"),
  alpha('frIndexedDate1 eq "notadate"', illegal("notadate", "/frIndexedDate1"), illegal("notadate", "/frIndexedDate1"), "a non-date literal is a 400 (measured)"),
  alpha('frUnindexedDate1 eq "notadate"', illegal("notadate", "/frUnindexedDate1"), illegal("notadate", "/frUnindexedDate1"), "a non-date literal is a 400 (measured)"),
  alpha('frIndexedDate1 ge "2026-01-01T00:00:00Z"', "fault", missingOn("alpha_user", "ge", "rule", "frIndexedDate1"), "ge was not probed"),
  alpha('frIndexedDate1 eq "2026-01-01"', "fault", missingOn("alpha_user", "eq", "rule", "frIndexedDate1"), "date-only literal was not probed"),
  alpha('frIndexedDate1 eq "2025-12-31T23:00:00.000Z"', "fault", missingOn("alpha_user", "eq", "rule", "frIndexedDate1"), "fractional seconds were not probed"),
  alpha('frIndexedDate1 eq "20251231230000Z"', "fault", missingOn("alpha_user", "eq", "rule", "frIndexedDate1"), "GeneralizedTime form was not probed"),
  alpha('frIndexedDate1 eq "2025-12-31T23:00:00"', "fault", missingOn("alpha_user", "eq", "rule", "frIndexedDate1"), "no zone: not probed, and not alphabetic, so not the measured 400"),
  alpha('frIndexedDate2 eq "2025-12-31T23:00:00Z"', "fault", missingOn("alpha_user", "eq", "rule", "frIndexedDate2"), "date 2-5 were not probed"),
  alpha('frIndexedDate1 eq "notadate" and frIndexedDate2 eq "x"', "fault", missingOn("alpha_user", "eq", "rule", "frIndexedDate2"),
    "an undecided leaf outranks a 400 elsewhere: which AIC reports first is unmeasured"),
  // §3e frIndexedInteger1: the literal is converted to the DS integer syntax. The AM lane
  // evaluates no ordering on an integer field (none was measured through that binding).
  alpha("frIndexedInteger1 eq 7", ["a"], ["a"], "control (measured)"),
  alpha('frIndexedInteger1 eq "7"', ["a"], ["a"], "quoted integer (measured)"),
  alpha("frIndexedInteger1 eq 7.0", ["a"], ["a"], "integral decimal (measured)"),
  alpha("frIndexedInteger1 eq 7.00", ["a"], ["a"], "integral decimal (measured over REST and IDM)"),
  alpha('frIndexedInteger1 eq "7.0"', ["a"], ["a"], "quoted integral decimal (measured over REST and IDM)"),
  alpha("frIndexedInteger1 ge 7.0", "fault", ["a", "b"], "ordering with an integral decimal (measured over REST and IDM)"),
  alpha("frIndexedInteger1 gt 7", "fault", ["b"], "ordering with an integer (assumed; AM does not evaluate gt here)"),
  alpha("frIndexedInteger1 eq 7.5", illegal("7.5", "/frIndexedInteger1"), illegal("7.5", "/frIndexedInteger1"), "non-integral: 400 (measured)"),
  alpha("frIndexedInteger1 lt 7.5", "fault", illegal("7.5", "/frIndexedInteger1"), "non-integral under ordering: 400 (measured over REST and IDM)"),
  alpha("frIndexedInteger1 gt 6.5", "fault", illegal("6.5", "/frIndexedInteger1"), "non-integral under ordering: 400 (measured over REST and IDM)"),
  alpha('frIndexedInteger1 eq "7.5"', illegal("7.5", "/frIndexedInteger1"), illegal("7.5", "/frIndexedInteger1"), "quoted non-integral: 400 (measured over REST and IDM)"),
  alpha('frIndexedInteger1 eq "abc"', illegal("abc", "/frIndexedInteger1"), illegal("abc", "/frIndexedInteger1"), "non-numeric: 400 (measured over REST and IDM)"),
  alpha("frIndexedInteger1 ge 7.5", "fault", missingOn("alpha_user", "ge", "type", "frIndexedInteger1"), "ge and le with a fraction were not probed"),
  alpha("frIndexedInteger1 eq 7.0e0", "fault", missingOn("alpha_user", "eq", "type", "frIndexedInteger1"), "an exponent was not probed"),
  alpha('frIndexedInteger1 gt "6"', "fault", missingOn("alpha_user", "gt", "type", "frIndexedInteger1"), "a quoted literal under ordering was not probed"),
  alpha('frIndexedInteger1 co "7"', "fault", missingOn("alpha_user", "co", "type", "frIndexedInteger1"), "substring on an integer was not probed"),
  alpha("frIndexedInteger1 eq true", "fault", missingOn("alpha_user", "eq", "type", "frIndexedInteger1"), "a boolean literal was not probed"),
  // Review pf-02 P3-1: CREST parses a number literal to a Double before DS sees it, so integrality
  // is the value's, not the spelling's. A quoted literal's conversion is unmeasured, so it is not.
  alpha("frIndexedInteger1 eq 7.00000000000000004", ["a"], ["a"], "CREST parses it to the Double 7.0: the same filter as eq 7.0"),
  alpha("frIndexedInteger1 eq 7.50000000000000001", illegal("7.5", "/frIndexedInteger1"), illegal("7.5", "/frIndexedInteger1"), "CREST parses it to the Double 7.5: the 400 for 7.5"),
  alpha('frIndexedInteger1 eq "7.0000000000000001"', "fault", missingOn("alpha_user", "eq", "type", "frIndexedInteger1"),
    "quoted, integral only after a Double parse IDM was not measured to make"),
  alpha('frIndexedInteger1 eq "7.5000000000000001"', "fault", missingOn("alpha_user", "eq", "type", "frIndexedInteger1"),
    "quoted, not the shortest spelling of its Double: whether IDM echoes it or 7.5 is unmeasured"),
  alpha('frIndexedInteger1 eq "7.50"', illegal("7.50", "/frIndexedInteger1"), illegal("7.50", "/frIndexedInteger1"), "quoted, trailing zeros aside it is 7.5: the 400, echoed as written"),
  alpha("frIndexedInteger1 eq 7.5 and frIndexedInteger1 eq 7", illegal("7.5", "/frIndexedInteger1"), illegal("7.5", "/frIndexedInteger1"), "the 400 is query-level, whatever the other leaf"),
  // Stored values outside the decided forms (review pf-02 P2-1): each is refused by its own guard.
  offForm({ telephoneNumber: "(02) 9999-9999" }, 'telephoneNumber eq "0299999999"', "eq", "rule", "telephoneNumber", "stored parentheses were not probed"),
  offForm({ telephoneNumber: 61299999999 }, 'telephoneNumber eq "61299999999"', "eq", "type", "telephoneNumber", "a stored number is not a telephone string"),
  offForm({ frIndexedDate1: "2026-01-01T00:00:00.000Z" }, 'frIndexedDate1 eq "2026-01-01T00:00:00Z"', "eq", "rule", "frIndexedDate1",
    "stored fractional seconds were not probed"),
  offForm({ frIndexedInteger1: "7" }, "frIndexedInteger1 eq 7", "eq", "type", "frIndexedInteger1", "a stored string is not the DS integer"),
  offForm({ frIndexedInteger1: 7.5 }, "frIndexedInteger1 eq 7", "eq", "type", "frIndexedInteger1", "a stored fraction is not the DS integer"),
  // Integer 2-5 and unindexed: the existing quoted-integer reading; other literals are refused before any record.
  alpha('frIndexedInteger2 eq "7"', "fault", ["a"], "assumed like frIndexedInteger1 (round 5); the AM lane has no quoted-integer reading"),
  alpha("frIndexedInteger2 eq 7.0", "fault", missingOn("alpha_user", "eq", "type", "frIndexedInteger2"), "integer 2-5 were not probed with decimals"),
  alpha('frUnindexedInteger1 eq "abc"', "fault", missingOn("alpha_user", "eq", "type", "frUnindexedInteger1"), "whether this is a 400 too was not probed"),
  // §3d: an explicitly written [] is not present.
  alpha("frIndexedMultivalued1 pr", ["b"], ["b"], "explicit [] reads as absent (measured)"),
  alpha("!(frIndexedMultivalued1 pr)", ["a", "c"], ["a", "c"], "measured"),
  alpha("frUnindexedMultivalued1 pr", ["b"], ["b"], "measured over REST and IDM; same DS rule"),
  alpha("!(frUnindexedMultivalued1 pr)", ["a", "c"], ["a", "c"], "measured over REST and IDM; same DS rule"),
  alpha("roles pr", ["b"], ["b"], "measured over REST and IDM (roles is a relationship on AIC; a plain array here)"),
  alpha("!(roles pr)", ["a", "c"], ["a", "c"], "measured over REST and IDM"),
  alpha("frIndexedMultivalued2 pr", "fault", missingOn("alpha_user", "pr", "presence", "frIndexedMultivalued2"), "multivalued 2-5 were not probed"),
  // bravo_user: nothing in §3 was probed there.
  bravo('telephoneNumber eq "+614 9999 9999"', "fault", missingOn("bravo_user", "eq", "rule", "telephoneNumber"), "not probed on bravo_user"),
  bravo('telephoneNumber eq "+614-9999-9999"', "fault", missingOn("bravo_user", "eq", "rule", "telephoneNumber"), "refused by name, even byte-equal"),
  bravo('frIndexedDate1 eq "2025-12-31T23:00:00Z"', "fault", missingOn("bravo_user", "eq", "rule", "frIndexedDate1"), "not probed on bravo_user"),
  bravo("frIndexedInteger1 eq 7.0", "fault", missingOn("bravo_user", "eq", "type", "frIndexedInteger1"), "not probed on bravo_user"),
  bravo("frIndexedInteger1 eq 7", ["a"], ["a"], "control: a plain integer is decided"),
  bravo("frIndexedMultivalued1 pr", "fault", missingOn("bravo_user", "pr", "presence", "frIndexedMultivalued1"), "not probed on bravo_user"),
  // §4 generic (JSON) storage, the probe's shape (`managed/*` only): a quoted literal never
  // matches a number; numbers compare by value.
  sink("count eq 7", ["s7"], ["s7"], "control (measured)"),
  sink('count eq "7"', "fault", [], "quoted never matches a number (measured)"),
  sink("count eq 7.0", "fault", ["s7"], "by value (measured)"),
  sink('count eq "7.0"', "fault", [], "measured"),
  sink("count eq 7.5", "fault", ["s75"], "a fraction is an ordinary value (measured)"),
  sink('count eq "7.5"', "fault", [], "measured"),
  sink("count gt 6.5", "fault", ["s7", "s8", "s75"], "measured"),
  sink('count gt "6"', "fault", [], "measured"),
  sink('count lt "10"', "fault", [], "measured"),
  sink("count le 7.5", "fault", ["s7", "s75"], "le by value (assumed like eq and gt)"),
  sink('!(count eq "7")', "fault", missingOn("sink", "eq", "type", "count"), "only a positive leaf was probed"),
  sink('count ge "7"', "fault", missingOn("sink", "ge", "type", "count"), "ge with a quoted literal was not probed"),
  // A type with its own genericMapping entry: decided like managed/*, labelled assumed (review P3-1).
  linkedsink('count eq "7"', "fault", [], "own genericMapping entry: assumed like managed/*"),
  linkedsink("count eq 7.0", "fault", ["l7"], "own genericMapping entry: assumed like managed/*"),
  linkedsink('linked eq "7"', "fault", missingOn("linkedsink", "eq", "type", "linked"), "a property the type's mapping stores in its own attribute is not JSON"),
  pinned('count eq "7"', "fault", missingOn("pinned", "eq", "type", "count"), "an explicitly mapped type is not JSON-stored"),
  pinned("count eq 7.0", "fault", missingOn("pinned", "eq", "type", "count"), "an explicitly mapped type is not JSON-stored"),
  pinned("count eq 7", ["p7"], ["p7"], "control"),
];
