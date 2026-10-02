/** Node's typed view of the canonical AM-safe policy asset; no layout data lives here. */
import { createRequire } from "node:module";
import { bindingsIdentityPolicyPath } from "../paths.ts";

type IdentityFamily = [
  RegExp,
  string,
  "single" | "multi",
  "integer" | "date" | null,
];
type IdentityLayout = {
  field: string;
  none: "remove" | "violation";
  one: "scalar";
  many: "array";
  syntax: null;
};

type IdentityPolicy = {
  families: IdentityFamily[];
  standard: Record<string, IdentityLayout>;
  collisionReason: (property: string) => string | null;
};

export const identityPolicy = createRequire(import.meta.url)(
  bindingsIdentityPolicyPath,
) as IdentityPolicy;
