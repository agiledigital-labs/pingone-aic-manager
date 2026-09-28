import { newStem, passId } from "./txid.ts";

/**
 * One AIC-lane run's transaction ids, parked for the vitest hook to pick up
 * if the test fails. Local-only tests never write one of these.
 */
export interface AicTrace {
  stem: string;
  passIds: string[];
  tenantName: string;
}

let current: AicTrace | undefined;

export function noteAicTrace(trace: AicTrace): void {
  current = {
    stem: trace.stem,
    passIds: [...trace.passIds],
    tenantName: trace.tenantName,
  };
}

export function appendAicPass(id: string): void {
  if (current === undefined) {
    throw new Error("rhino-local: appendAicPass without an active AIC trace");
  }
  current.passIds.push(id);
}

export function peekAicTrace(): AicTrace | undefined {
  if (current === undefined) {
    return undefined;
  }
  return {
    stem: current.stem,
    passIds: [...current.passIds],
    tenantName: current.tenantName,
  };
}

export function takeAicTrace(): AicTrace | undefined {
  const out = peekAicTrace();
  current = undefined;
  return out;
}

export function clearAicTrace(): void {
  current = undefined;
}

/** Subject chain: numbered passes under one stem. */
export function beginTrace(
  tenantName: string,
  stem = newStem()
): { stem: string; next: () => string } {
  noteAicTrace({ stem, passIds: [], tenantName });
  let step = 0;
  return {
    stem,
    next() {
      step += 1;
      const id = passId(stem, step);
      appendAicPass(id);
      return id;
    },
  };
}

/**
 * A one-off authenticate that is not a numbered subject pass (the session
 * minter). Published so a mint failure is still look-up-able; the subject
 * trace replaces it if minting succeeds.
 */
export function beginSingleTrace(tenantName: string, id = newStem()): string {
  noteAicTrace({ stem: id, passIds: [id], tenantName });
  return id;
}
