import { parseIdentityCustomAttrs } from "../case/identity.ts";
import { isPlainObject } from "../case/util.ts";
import type { JsonObject } from "../case/types.ts";
import { checkRecord, SchemaViolation } from "./validate.ts";
import type { EnvProfile } from "./types.ts";

/**
 * Strict-check every seeded managed record against the environment schema.
 *
 * Runs in Node before the case reaches the JVM, so a fixture typo is reported
 * against the case that owns it. Records are checked in `seed` mode: property
 * names and enum values must be real, but `required` is not demanded — a
 * fixture legitimately carries only the fields the script under test reads.
 */
export function checkSeededManaged(
  profile: EnvProfile,
  managed: Record<string, JsonObject[]> | undefined,
  caseName: string,
  identityCustomAttrs: Record<string, string[]> = {},
): void {
  if (managed === undefined) {
    return;
  }
  const bags = parseIdentityCustomAttrs(
    identityCustomAttrs,
    managed,
    "given.identityCustomAttrs",
  );
  for (const [collection, rows] of Object.entries(managed)) {
    for (const [index, row] of rows.entries()) {
      const id = typeof row._id === "string" ? row._id : String(index);
      try {
        const values = bags[`${collection}/${id}`];
        const bag: unknown =
          values?.length === 1 ? JSON.parse(values[0] as string) : {};
        // Proven bag writes bypass schema checks; ordinary seeded fields stay strict.
        const ordinary = Object.fromEntries(
          Object.entries(row).filter(
            ([key]) => !isPlainObject(bag) || !Object.hasOwn(bag, key),
          ),
        );
        checkRecord(profile, `${collection}/${id}`, ordinary, "seed");
      } catch (error) {
        if (error instanceof SchemaViolation) {
          throw new SchemaViolation(
            error.kind,
            error.resource,
            `case ${JSON.stringify(caseName)}: given.managed[${JSON.stringify(collection)}][${index}]: ${error.message.replace(/^rhino-local: /, "")}`,
          );
        }
        throw error;
      }
    }
  }
}
