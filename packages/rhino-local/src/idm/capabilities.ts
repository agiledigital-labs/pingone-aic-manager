/** Refuse behavior-bearing schemas the repository cannot faithfully execute. */
import { isDocument } from "./document.ts";
import { missing } from "./errors.ts";
import type { Evidence } from "./provenance.ts";
import type { ManagedType } from "./types.ts";

// These describe display/configuration or behavior handled by the core/policy seam.
// Unknown declarations fail closed rather than silently becoming metadata.
const supported = new Set(["type", "properties", "items", "default", "required", "enum", "policies",
  "onValidate", "onStore", "onRetrieve", "title", "description", "order", "options", "iconClass", "searchable"]);
export function checkCapabilities(type: ManagedType, writing: boolean, evidence: Evidence, hasPolicy: boolean): void {
  const visit = (config: Record<string, unknown>, path: string): void => {
    if (config.type === "relationship") missing(evidence, `managed relationship: ${path}`);
    if (Object.hasOwn(config, "type") && (typeof config.type !== "string" || !["string", "number", "boolean", "object", "array"].includes(config.type))) {
      missing(evidence, `property behavior type: ${path}`);
    }
    for (const key of Object.keys(config)) {
      if (["encryption", "secureHash"].includes(key) && !writing) continue;
      if (writing && !hasPolicy && ["enum", "policies", "required"].includes(key) &&
          (key !== "required" || !Array.isArray(config[key]) || (config[key] as unknown[]).length > 0)) {
        missing(evidence, `schema policy ${key}: ${path}`);
      }
      if (supported.has(key)) continue;
      // Even false-valued flags have unmeasured semantics; don't guess a no-op.
      missing(evidence, `property behavior ${key}: ${path}`);
    }
    if (Object.hasOwn(config, "searchable")) evidence.measured("searchable is UI/schema metadata, not a repository index", "docs/api/10-managed-objects.md");
    if (isDocument(config.properties)) for (const [name, child] of Object.entries(config.properties)) {
      if (isDocument(child)) visit(child, `${path}/${name}`);
    }
    if (isDocument(config.items)) visit(config.items, `${path}/items`);
  };
  visit(type.schema, type.name);
}
