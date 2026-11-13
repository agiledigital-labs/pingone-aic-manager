/**
 * Failed tests whose error is a case-validation refusal, as one audit the
 * fault tally already knows how to count. Caught refusals do not fail the
 * test, so they are not here.
 */
import type { EmulationAudit } from "../emulation.ts";
import { validationFaultMessage, validationFaultOf, type CaseValidationFaultId } from "../case/validation-fault.ts";

export interface ValidationFailure {
  file: string;
  /** Stable per test, so two tests count as two cases. */
  ownerId: string;
  ownerName: string;
  errors: ReadonlyArray<{ name?: string; cause?: unknown }>;
}

export function caseValidationAudit(failures: readonly ValidationFailure[]): EmulationAudit | undefined {
  const entries: EmulationAudit["failures"] = [];
  let sequence = 0;
  for (const failure of failures) {
    const seen = new Set<CaseValidationFaultId>();
    const faults: { id: string; message: string }[] = [];
    for (const error of failure.errors) {
      const id = validationFaultOf(error);
      if (id === undefined || seen.has(id)) continue;
      seen.add(id);
      faults.push({ id, message: validationFaultMessage(id) });
    }
    if (faults.length === 0) continue;
    entries.push({
      sequence: sequence++,
      context: "case validation",
      file: failure.file,
      owner: { id: failure.ownerId, name: failure.ownerName, kind: "test", file: failure.file },
      causes: faults.map((fault) => `${fault.id}: ${fault.message}`),
      faults,
    });
  }
  if (entries.length === 0) return undefined;
  return { id: "case-validation", revision: 1, failures: entries, outstanding: [] };
}
