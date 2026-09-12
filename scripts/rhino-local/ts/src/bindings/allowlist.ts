import { readFileSync } from "node:fs";
import { bindingsJsonPath } from "../paths.ts";

let cached: string[] | undefined;

/**
 * Boxed value types AM permits that its declared `allowLists` does not name.
 *
 * NOT from the descriptor, and kept separate from it so the audit trail shows
 * which half of the policy is documented and which is not.
 *
 * The descriptor names `java.lang.Byte`, `Character`, `Float`, `Long`,
 * `Number` and `Short` — but not `Double`, `Integer`, `Boolean` or `String`.
 * That reads as an omission in AM's descriptor rather than a policy, because
 * enforcing it would break rows the live matrix records as WORKING:
 *
 * - `java.util.Collections.*` is a live ✅ on the decision node, yet
 *   `Collections.singletonMap("a", 1).get("a")` needs `java.lang.Double` —
 *   Rhino converts a JS number to a Double. Without it the call fails with
 *   `Access to Java class "java.lang.Double" is prohibited`.
 * - An index loop is what `docs/api/12-script-bindings-matrix.md` calls "the
 *   portable answer" for a Java List, and `list.get(0)` on a list of strings
 *   hands back a `java.lang.String`.
 *
 * The same page records the matching anomaly in the other context:
 * `java.lang.String` and `java.lang.Integer` resolve in validate-scope although
 * neither is declared there — "the rule behind that split is not established".
 *
 * Scoped to the types Rhino produces converting JS primitives across the
 * boundary, rather than a speculative widening.
 *
 * STATUS: inferred from live-recorded rows, NOT directly measured on a decision
 * node. `scripts/rhino-local/ts` has a probe ready; measure and then either
 * fold these into the descriptor's story or drop them.
 */
export const OBSERVED_BASE: readonly string[] = [
  "java.lang.String",
  "java.lang.Double",
  "java.lang.Integer",
  "java.lang.Boolean",
];

/**
 * The scripted decision node's Java class allow-list: the descriptor's declared
 * entries plus `OBSERVED_BASE`.
 *
 * Read from the binding descriptor this project already extracted from AM
 * (`docs/api/bindings/scripted-decision-next.json`), so the policy has one
 * source and cannot drift from the document the rest of the harness generates
 * its mocks from. Not hand-transcribed: a copied list is a list that goes
 * stale silently.
 */
export function decisionNodeClassAllowList(): string[] {
  if (cached === undefined) {
    const parsed: unknown = JSON.parse(readFileSync(bindingsJsonPath, "utf8"));
    const lists =
      typeof parsed === "object" && parsed !== null
        ? (parsed as { allowLists?: unknown }).allowLists
        : undefined;
    if (!Array.isArray(lists) || lists.some((item) => typeof item !== "string")) {
      throw new Error(
        `rhino-local: ${bindingsJsonPath} has no allowLists array of strings`
      );
    }
    cached = [...(lists as string[]), ...OBSERVED_BASE];
  }
  return cached;
}
