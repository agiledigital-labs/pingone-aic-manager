/** Framework-free host callback evidence, isolated by logical async call chain. */
import { AsyncLocalStorage } from "node:async_hooks";
import type { BehaviorEvidence, JobResponse } from "./protocol.ts";
const current = new AsyncLocalStorage<BehaviorEvidence[]>();

export function withHostEvidence<T>(entries: BehaviorEvidence[], fn: () => T): T {
  return current.run(entries, fn);
}
export function reportHostEvidence(entries: readonly BehaviorEvidence[]): void {
  const target = current.getStore();
  if (!target) return;
  for (const entry of entries) {
    const prior = target.find((item) => item.kind === entry.kind && item.behavior === entry.behavior);
    if (!prior) target.push({ ...entry });
    else if (!prior.source && entry.source) prior.source = entry.source;
  }
}
/** Nested results still contribute when an adapter ignores their return value. */
export function reportResponseEvidence(response: JobResponse): void {
  reportHostEvidence(response.provenance ?? []);
  reportHostEvidence((response.assumed ?? []).map((behavior) => ({ kind: "assumed", behavior })));
  reportHostEvidence((response.measured ?? []).map((behavior) => ({ kind: "measured", behavior })));
}
