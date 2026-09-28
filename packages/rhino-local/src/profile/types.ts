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
export interface EnvProfile {
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
}

export interface PropertySchema {
  /**
   * Normalised JSON-schema type. IDM writes `["string","null"]` for a nullable
   * scalar; that becomes `type: "string"` plus `nullable: true`, so every
   * consumer sees one spelling.
   */
  type: string;
  nullable?: boolean;
  /** Present only on an enum-constrained scalar. AIC enforces this on write. */
  enum?: JsonValue[];
  /** Element schema for `type: "array"`. */
  items?: PropertySchema;
  /** `managed/<target>` paths, for `type: "relationship"`. */
  resourceCollection?: string[];
}
