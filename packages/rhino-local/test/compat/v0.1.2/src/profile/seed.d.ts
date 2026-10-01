import type { JsonObject } from "../case/types.ts";
import type { EnvProfile } from "./types.ts";
/**
 * Strict-check every seeded managed record against the environment schema.
 *
 * Runs in Node before the case reaches the JVM, so a fixture typo is reported
 * against the case that owns it. Records are checked in `seed` mode: property
 * names and enum values must be real, but `required` is not demanded — a
 * fixture legitimately carries only the fields the script under test reads.
 */
export declare function checkSeededManaged(profile: EnvProfile, managed: Record<string, JsonObject[]> | undefined, caseName: string): void;
