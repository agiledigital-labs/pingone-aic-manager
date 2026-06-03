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

// Union of the Java List/array methods scripts use across families (next-gen
// uses `length`/`includes`; legacy Java collections use `size`/`contains`).
interface JavaArray<T = JavaString> {
  [index: number]: T | null | undefined;
  length: number;
  size(): number;
  get(index: number): T | null | undefined;
  includes(value: T): boolean;
  contains(value: T): boolean;
  isEmpty(): boolean;
  asList(): any[];
  toArray(): T[];
}

interface JavaMap<Key = JavaString, Value = JavaString> {
  get(key: Key): Value | null;
  containsKey?(key: Key): boolean;
}

interface JavaSet<T = JavaString> {
  contains(key: T): boolean;
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
