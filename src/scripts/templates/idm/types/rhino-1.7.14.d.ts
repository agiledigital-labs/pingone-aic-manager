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

// A java.util.Set surfaced into Rhino. It is NOT a JS array and NOT a JS Set:
// there is no `.includes`, no `.length`, and no `.has`. Membership is
// `.contains(x)`. Mirrors the AM-side JavaSet so both workspaces read alike.
interface JavaSet<T = JavaString> {
  contains(key: T): boolean;
  size(): number;
  toArray(): T[];
  isEmpty(): boolean;
}
