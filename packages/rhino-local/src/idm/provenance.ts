/** Request-local provenance; nested host calls share it, independent requests do not. */
import { reportHostEvidence } from "../host-evidence.ts";
import { AsyncLocalStorage } from "node:async_hooks";
import type { Provenance } from "./types.ts";

export class Evidence {
  private readonly local = new AsyncLocalStorage<Provenance[]>();

  async run<T>(fn: () => Promise<T>): Promise<{ value: T; provenance: Provenance[] }> {
    const entries: Provenance[] = [];
    const value = await this.local.run(entries, fn);
    return { value, provenance: entries };
  }

  bind<A extends unknown[], T>(fn: (...args: A) => T): (...args: A) => T {
    const entries = this.local.getStore();
    if (!entries) throw new Error("IDM provenance scope is absent");
    return (...args) => this.local.run(entries, () => fn(...args));
  }

  add(kind: Provenance["kind"], behavior: string, source?: string): void {
    const entries = this.local.getStore();
    if (!entries) throw new Error("IDM provenance scope is absent");
    const entry: Provenance = { kind, behavior, ...(source ? { source } : {}) };
    if (!entries.some((entry) => entry.kind === kind && entry.behavior === behavior)) entries.push(entry);
    // Publish even if this request's list already had it: a nested evaluation
    // may be collecting the same behavior in a different host callback.
    reportHostEvidence([entry]);
  }

  measured(behavior: string, source = "docs/api/24-idm-script-runtime.md"): void {
    this.add("measured", behavior, source);
  }

  assumed(behavior: string): void {
    this.add("assumed", behavior);
  }

  firstMissing(): Provenance | undefined {
    return this.local.getStore()?.find((entry) => entry.kind === "missing");
  }
}
