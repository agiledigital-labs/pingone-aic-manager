import type { JsonValue } from "../case/types.ts";

/**
 * A normalised snapshot of one environment's managed-object schema.
 *
 * Deliberately NOT the raw `/openidm/config/managed` document. That document
 * carries lifecycle hook SOURCE BODIES, which are client code, and a great deal
 * of UI metadata (`viewable`, `searchable`, `iconClass`, `order`) that no
 * validation here consults. Normalising keeps the artefact small enough to read
 * in a diff and drops the two things we must not accumulate: script source and
 * presentation noise.
 *
 * Lives under `workspace/<tenant>/`, which `.gitignore` covers in full — object
 * and property names are client business vocabulary.
 */
/**
 * The profile format this package writes. 1 (or absent) predates hook, default and conditional-
 * policy recording, so the script tester cannot tell a type with none from one pulled before they
 * were recorded, and faults its writes (`openidm-managed-profile-stale`). Bump it whenever a field
 * becomes one whose absence means "none". 3 (sol-r2 P2-1) records a property's nested
 * `properties` and keeps a type list that is not one type plus `"null"` as `types`: a version-2
 * profile turned `["string", "number"]` into `string` and dropped nested declarations, so a nested
 * or multi-type `default` cannot be judged from it.
 */
export const PROFILE_VERSION = 3;

export interface EnvProfile {
  /** The format version, {@link PROFILE_VERSION} when pulled by this package. Absent before 2. */
  profileVersion?: number;
  /** Tenant context name, as `aic ctx list` reports it. Never a hostname. */
  tenant: string;
  /** ISO-8601. Staleness is the operator's call; nothing here expires. */
  pulledAt: string;
  /** Endpoints this profile was built from, for the audit trail. */
  sources: string[];
  /** Keyed by managed object name, e.g. `alpha_user`. */
  objects: Record<string, ObjectSchema>;
}

export interface ObjectSchema {
  name: string;
  properties: Record<string, PropertySchema>;
  /** Property names IDM rejects a create without. */
  required: string[];
  /**
   * True when the schema has no `required` array at all, which stock `policy.js` treats differently
   * from an empty one (a null is then also `VALID_TYPE`). Absent means the array was there.
   */
  noRequiredArray?: true;
  /**
   * Names of the object-level script hooks the type declares (`onCreate`, `postUpdate`, …), sorted,
   * WITHOUT their bodies: hook source is client code. Always written from version 2; a version-2
   * profile that omits it declares none.
   */
  hooks?: string[];
}

export interface PropertySchema {
  /**
   * Normalised JSON-schema type. IDM writes `["string","null"]` for a nullable
   * scalar; that becomes `type: "string"` plus `nullable: true`, so every
   * consumer sees one spelling.
   */
  type: string;
  nullable?: boolean;
  /**
   * The declared type list verbatim, when it is not one type plus `"null"` (`["string", "number"]`);
   * `type` is then `any`. A default on such a property is judged against this list, which no probe
   * covered, so it faults as unmeasured rather than as whichever type came first.
   */
  types?: unknown[];
  /** Present only on an enum-constrained scalar. AIC enforces this on write. */
  enum?: JsonValue[];
  /** Element schema for `type: "array"`. */
  items?: PropertySchema;
  /**
   * Member schemas for `type: "object"`, normalised the same way. Recorded so a nested `default`
   * can be judged; nested values are not checked on write (AIC stores them unenforced, docs/api/10).
   */
  properties?: Record<string, PropertySchema>;
  /** `managed/<target>` paths, for `type: "relationship"`. */
  resourceCollection?: string[];
  /** Property-level script hooks (`onValidate`, `onStore`, `onRetrieve`), names only, sorted. */
  hooks?: string[];
  /** The `policyId`s of the property's declared `policies`, in order. AIC runs them on write. */
  policies?: string[];
  /**
   * The property's schema `default`, when it declares one (`null` included). Schema config, not
   * client code. AIC applies it on a create that omits the property and, for a non-null default,
   * refuses an explicit `null` (`NOT_NULL`, docs/api/10-managed-objects.md).
   */
  default?: JsonValue;
  /**
   * The `policyId`s inside the property's `conditionalPolicies` entries, and its `fallbackPolicies`,
   * which stock `policy.js` runs when no condition matched. Names only.
   */
  conditionalPolicies?: string[];
  fallbackPolicies?: string[];
}
