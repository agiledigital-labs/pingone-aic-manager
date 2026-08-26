// Mozilla Rhino 1.7.14 interop scaffolding, shared by ALL AM scripts.
//
// AIC runs AM scripts on Rhino 1.7.14 for both the legacy and next-generation
// engines (verified — see docs/api/12-script-bindings-matrix.md). Rhino surfaces
// Java objects (strings, arrays, collections) to JavaScript; these interfaces
// describe the Java-flavoured shapes scripts actually touch.
//
// This file declares ONLY types and global augmentations — never a runtime
// binding — so it can be included by every leaf without clashing with the
// per-family binding overlays.

type StringLike = string | JavaString;

interface JavaString {
  new (value: StringLike): JavaString;
  includes(value: StringLike): boolean;
  split(separator: StringLike): JavaArray<JavaString>;
  getAsUtf8(): JavaString;
  getBytes(): JavaByteArray;
}

interface JavaByteArray {
  new (value: StringLike): JavaByteArray;
}

// The argument to a Java lookup. Rhino converts a JS string on the way in, so
// `scopes.contains("openid")` is the ordinary way to write it and typing the
// parameter as the collection's own element type rejected all of them. The
// conditional keeps that widening where the element really is a Java string:
// `JavaArray<number>.includes("2")` is still an error, and so is
// `JavaArray<Claim>.contains("name")`.
type Lookup<T> = T extends JavaString ? StringLike : T;

// Union of the Java List/array methods scripts use across families (next-gen
// uses `length`/`includes`; legacy Java collections use `size`/`contains`).
interface JavaArray<T = JavaString> {
  [index: number]: T | null | undefined;
  length: number;
  size(): number;
  get(index: number): T | null | undefined;
  includes(value: Lookup<T>): boolean;
  contains(value: Lookup<T>): boolean;
  isEmpty(): boolean;
  asList(): any[];
  toArray(): T[];
}

interface JavaMap<Key = JavaString, Value = JavaString> {
  get(key: Lookup<Key>): Value | null;
  containsKey?(key: Lookup<Key>): boolean;
}

// Request header/parameter/cookie bindings are Java multimaps surfaced without
// enumerated methods in the editor metadata; this is the shape scripts use.
interface RequestMap {
  get(key: StringLike): JavaArray<JavaString> | null;
  containsKey(key: StringLike): boolean;
}

interface JavaSet<T = JavaString> {
  contains(key: Lookup<T>): boolean;
  size(): number;
  toArray(): T[];
  isEmpty(): boolean;
}

// Rhino lets a JS Array concat Java arrays; widen the lib signature so scripts
// that concat a JavaArray<T> into a JS array still type-check.
interface Array<T> {
  concat<U extends T[]>(...items: U[]): T[];
  concat(...items: (T | JavaArray<T>)[]): T[];
}
