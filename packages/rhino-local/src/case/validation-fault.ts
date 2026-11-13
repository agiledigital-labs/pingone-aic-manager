/**
 * Case-validation refusals, one stable id per kind. The thrown error keeps
 * the detailed message (paths and values). The tally and the timing row use
 * the stable message, which does not repeat those values.
 *
 * The id rides on `error.name` as `CaseValidationError/<id>`. Vitest keeps
 * `name` when it reports a failed test or suite hook to the main process,
 * and drops other properties, so the reporter reads the id from the name.
 */

export const CASE_VALIDATION_FAULTS = {
  "case-invalid/engine-legacy": "the legacy (V1) script engine is no longer emulated",
  "case-invalid/engine": "the case names an engine other than next-gen",
  "case-invalid/unknown-key": "the case has an unknown key",
  "case-invalid/context": "the case sets a value this script context does not accept",
  "case-invalid/conflict": "the case sets values that cannot be combined",
  "case-invalid/duplicate": "the case lists a value twice",
  "case-invalid/required": "the case is missing a required value",
  "case-invalid/not-object": "the case is not an object",
  "case-invalid/empty": "the case has an empty value",
  "case-invalid/wrong-type": "the case has a value of the wrong type",
  "case-invalid/value": "the case has a value the harness refuses",
} as const;

export type CaseValidationFaultId = keyof typeof CASE_VALIDATION_FAULTS;

const NAME_PREFIX = "CaseValidationError/";

export class CaseValidationError extends Error {
  readonly faultId: CaseValidationFaultId;
  constructor(faultId: CaseValidationFaultId, message: string) {
    super(message);
    this.faultId = faultId;
    this.name = `${NAME_PREFIX}${faultId}`;
  }
}

export function validationFaultMessage(id: CaseValidationFaultId): string {
  return CASE_VALIDATION_FAULTS[id];
}

/** Kind of a validation message. Specific kinds win over the general ones. */
export function validationFaultId(message: string): CaseValidationFaultId {
  if (message.includes("no longer emulated")) return "case-invalid/engine-legacy";
  if (message.includes('must be "next-gen"')) return "case-invalid/engine";
  if (message.includes("has unknown key")) return "case-invalid/unknown-key";
  if (message.includes("not accepted")) return "case-invalid/context";
  if (
    message.includes("collides") ||
    message.includes("cannot be declared with") ||
    message.includes("together with") ||
    message.includes("and not both")
  ) {
    return "case-invalid/conflict";
  }
  if (message.includes(" twice")) return "case-invalid/duplicate";
  if (message.includes("is required") || message.includes("must declare")) return "case-invalid/required";
  if (
    message.includes("is not an object") ||
    message.includes("must be an object") ||
    message.includes("case is not an object")
  ) {
    return "case-invalid/not-object";
  }
  if (
    message.includes("must not be empty") ||
    message.includes("non-empty") ||
    message.includes("has an empty") ||
    message.includes("must name an export")
  ) {
    return "case-invalid/empty";
  }
  if (message.includes("must be") || message.includes("is not an array") || message.includes("must be a")) {
    return "case-invalid/wrong-type";
  }
  return "case-invalid/value";
}

export function validationFaultFromName(name: string | undefined): CaseValidationFaultId | undefined {
  if (name === undefined || !name.startsWith(NAME_PREFIX)) return undefined;
  const id = name.slice(NAME_PREFIX.length);
  return Object.hasOwn(CASE_VALIDATION_FAULTS, id) ? (id as CaseValidationFaultId) : undefined;
}

/** The id on this error or on a cause Vitest wrapped it in. */
export function validationFaultOf(error: { name?: string; cause?: unknown }, depth = 0): CaseValidationFaultId | undefined {
  const id = validationFaultFromName(error.name);
  if (id !== undefined) return id;
  const cause = error.cause;
  if (depth < 4 && cause !== null && typeof cause === "object" && "name" in cause) {
    return validationFaultOf(cause as { name?: string; cause?: unknown }, depth + 1);
  }
  return undefined;
}
