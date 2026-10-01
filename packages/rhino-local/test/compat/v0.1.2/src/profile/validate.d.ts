import type { JsonObject } from "../case/types.ts";
import type { EnvProfile, ObjectSchema } from "./types.ts";
export declare class SchemaViolation extends Error {
    readonly kind: "unknown-type" | "unknown-property" | "missing-required" | "bad-enum";
    readonly resource: string;
    constructor(kind: SchemaViolation["kind"], resource: string, message: string);
}
/** `managed/alpha_user/alice` -> `{ type: "managed/alpha_user", id: "alice" }`. */
export declare function splitResource(resource: string): {
    type: string;
    id?: string;
};
/** Object name for a `managed/<name>` resource, or undefined for other containers. */
export declare function managedObjectName(resourceType: string): string | undefined;
/**
 * Does this environment define the object type behind `resource`?
 *
 * This is the whole reason a profile is worth pulling. Without one, the harness
 * cannot tell "the tenant has this type and the record is simply absent"
 * (AIC returns `null`) from "this type does not exist and your fixture has a
 * typo" (AIC also returns `null`, unhelpfully). With one, the two become
 * distinguishable locally, and only the second is worth failing on.
 */
export declare function knowsType(profile: EnvProfile, resource: string): boolean;
export declare function objectFor(profile: EnvProfile, resource: string): ObjectSchema | undefined;
/** Names a type that is not in the profile, listing near misses. */
export declare function unknownTypeError(profile: EnvProfile, resource: string): SchemaViolation;
/**
 * Strict check of a record's properties against the object schema.
 *
 * `mode` picks which half of the rule applies. A seeded fixture is checked for
 * property existence only — it stands in for a record the tenant already holds,
 * and demanding `required` of it would reject every partial fixture. A write is
 * checked for `required` too, because that is what AIC enforces on create.
 */
export declare function checkRecord(profile: EnvProfile, resource: string, record: JsonObject, mode: "seed" | "create" | "patch"): void;
/** True when `value` is a plain record we can check. */
export declare function isRecord(value: unknown): value is JsonObject;
