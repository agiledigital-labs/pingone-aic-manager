// Mozilla Rhino 1.7.14 interop scaffolding, shared by ALL IDM scripts.
// Types and global augmentations only — no runtime bindings — so it composes
// with the per-family binding overlays without clashing.
//
// The slf4j logger format types at the foot of this file MIRROR the AM-side
// copy in `am/types/rhino-1.7.14.d.ts` deliberately, so one mental model covers
// both workspaces. Read the evidence note there; the difference in what is
// verified is recorded below.

type StringLike = string | JavaString;

interface JavaString {
  new (value: StringLike): JavaString;
  includes(value: StringLike): boolean;
  split(separator: StringLike): JavaArray<JavaString>;
}

interface JavaArray<T = JavaString> {
  [index: number]: T | null | undefined;
  length: number;
  includes(value: T): boolean;
  asList(): any[];
}

interface JavaMap<Key = JavaString, Value = JavaString> {
  get(key: Key): Value | null;
}

// A java.util.Set surfaced into Rhino. It is NOT a JS array and NOT a JS Set:
// there is no `.includes`, no `.length`, and no `.has`. Membership is
// `.contains(x)`. Mirrors the AM-side JavaSet so both workspaces read alike.
interface JavaSet<T = JavaString> {
  contains(key: T): boolean;
  size(): number;
  toArray(): T[];
  isEmpty(): boolean;
}

// ---- logger format strings ------------------------------------------------
//
// IDM's `logger` is slf4j-backed like AM's, so the first argument is a FORMAT
// STRING and every `{}` in it is filled from the next extra argument.
//
// PROVENANCE, and it differs from the AM copy: the formatting rules below were
// verified on both AM engines on 2026-08-27 with live probes
// (`fixtures-legacy/legacy-logger-levels.script.js`,
// `fixtures/logger-placeholders.script.js`). On IDM they are NOT probe-verified
// — a custom endpoint calling `logger.error`/`warn`/`info`/`debug` produced no
// entry in `idm-core` or `idm-everything` at all, so there was nothing to read
// the formatted line out of (2026-08-27, `endpoint/aicprobe-logger`, created
// and deleted). The types are applied here on the documented slf4j behaviour
// and the AM measurement, at the maintainer's direction. If IDM turns out to
// format differently, this file is the thing to change — see
// docs/api/12-script-bindings-matrix.md.
//
// A count mismatch is silent at runtime, in both directions:
//
//   logger.error("user {} in realm {}", user)  ->  "user alice in realm {}"
//   logger.error("user not found", user)       ->  "user not found"  (arg dropped)
//
// Both are bugs you only find by reading the log you wrote the line to avoid
// reading, so the types below COUNT the `{}` and make the compiler ask for one
// argument each. A format string that has widened to `string` cannot be counted
// and falls back to the old unchecked variadic.

/**
 * A Java `Throwable` as Rhino surfaces it — a caught Java exception, or one
 * built with `new java.lang.RuntimeException(...)` on the legacy engine (the
 * next-gen allow-list refuses to construct one; catch it from a static call
 * instead).
 *
 * Members are the ones a script reads. The point of the type is to be something
 * a stray string or number is NOT assignable to, which is what makes
 * `logger.error("hello", "world")` an error.
 *
 * A JavaScript `Error` is deliberately NOT one of these. Verified 2026-08-27:
 * `logger.error("js error {} bound", "X", new Error("boom"))` logged
 * `js error X bound` with **no** `exception` field — slf4j dropped the `Error`
 * as a surplus argument rather than treating it as a throwable. Typing it as a
 * throwable would have licensed a call whose second half goes nowhere.
 */
interface JavaThrowable {
  getMessage(): StringLike | null;
  getLocalizedMessage(): StringLike | null;
  getCause(): JavaThrowable | null;
  getStackTrace(): JavaArray<any>;
  printStackTrace(): void;
}

/**
 * Does `S` end in an ODD number of backslashes — i.e. is the `{}` that follows
 * it escaped?
 *
 * Parity, not presence. slf4j reads `\\` as one literal backslash, so
 * `"a \\\\{} b"` in JS source reaches the runtime as `a \\{} b` and the `{}` is
 * a REAL placeholder. Verified 2026-08-27 (`AICPROBE-G1`): it logged
 * `double \X bound`, binding the argument. A check that looked at one
 * backslash would call that escaped and reject a correct call.
 */
type EndsInOddBackslashes<
  S extends string,
  Odd extends boolean = false,
> = S extends `${infer Rest}\\`
  ? EndsInOddBackslashes<Rest, Odd extends true ? false : true>
  : Odd;

/**
 * One `unknown` slot per UNESCAPED `{}` in `S`. `\{}` is an escape: slf4j
 * leaves a literal `{}` in the output and binds nothing (verified 2026-08-27 —
 * `"escaped \\{} then {} bound"` with one argument logged
 * `escaped {} then X bound`).
 */
type LogPlaceholders<
  S extends string,
  Acc extends unknown[] = [],
> = S extends `${infer Head}{}${infer Rest}`
  ? EndsInOddBackslashes<Head> extends true
    ? LogPlaceholders<Rest, Acc>
    : LogPlaceholders<Rest, [...Acc, unknown]>
  : Acc;

/**
 * The extra arguments a call with format string `S` may pass: exactly one per
 * `{}`, optionally followed by a throwable.
 *
 * It is a UNION of two tuples rather than one tuple with an optional tail,
 * because an optional element also accepts an explicit `undefined` —
 * `logger.error("boom", undefined)` would have compiled while the runtime
 * dropped the argument.
 *
 * A format string that has WIDENED to `string` cannot be counted, and
 * `string extends S` is what detects that. Without the guard the pattern match
 * simply fails and the call is typed as taking NO extra arguments, which would
 * reject every dynamically-built message. What widens: a `var`/`let` holding a
 * string, a concatenation, a `JavaString`. What does NOT: a template literal,
 * whose type stays `` `user ${string}` `` — so `{}` written inside one is still
 * counted, which is right, since `${}` interpolation and `{}` binding are
 * different mechanisms and a script can use both in one line.
 *
 * Two things this deliberately does NOT model, both measured rather than
 * assumed:
 *
 * - **A trailing throwable does not fill a placeholder.** slf4j strips it
 *   BEFORE formatting, unconditionally — `logger.error("failed {}", throwable)`
 *   logs `failed {}` with the braces intact plus an `exception` field (verified
 *   2026-08-27, `AICPROBE-H1`/`H2`). The type does not catch that for an
 *   `any`-typed throwable, which today is all of them: the workspace sets
 *   `useUnknownInCatchVariables: false` and `java.*` is `any`, and no static
 *   shape distinguishes that from an ordinary `any` without rejecting both. A
 *   statically typed throwable COULD be caught by inferring the argument tuple
 *   and stripping a known-throwable tail; it is not worth the machinery while
 *   no such value exists here. Pass the message you want, then the throwable:
 *   `logger.error("failed {}", reason, e)`.
 * - **An open template-literal hole is treated as containing no `{}`.** If the
 *   interpolated value itself carries `{}` at runtime, the count is short. So
 *   is a branded string subtype or an unresolved generic parameter, neither of
 *   which `string extends S` recognises as dynamic.
 */
type LogArgs<S extends StringLike> = string extends S
  ? unknown[]
  : S extends string
    ? LogPlaceholders<S> | [...LogPlaceholders<S>, JavaThrowable]
    : unknown[];

/**
 * An slf4j-style log method. Generic in the format string so the `{}` in it can
 * be counted; see `LogArgs`.
 */
type LogFunction = <S extends StringLike>(
  message: S,
  ...args: LogArgs<S>
) => void;
