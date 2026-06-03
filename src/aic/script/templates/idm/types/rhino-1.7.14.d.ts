// Mozilla Rhino 1.7.14 interop scaffolding, shared by ALL IDM scripts.
// Types and global augmentations only — no runtime bindings — so it composes
// with the per-family binding overlays without clashing.

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
