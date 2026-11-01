/** Request-local provenance; nested host calls share it, independent requests do not. */
import { AsyncLocalStorage } from "node:async_hooks";
import type { Provenance } from "./types.ts";

export class Evidence {
  private readonly local = new AsyncLocalStorage<Provenance[]>();

  async run<T>(fn: () => Promise<T>): Promise<{ value: T; provenance: Provenance[] }> {
    const entries: Provenance[] = [];
    const value = await this.local.run(entries, fn);
    return { value, provenance: entries };
  }

  add(kind: Provenance["kind"], behavior: string, source?: string): void {
    const entries = this.local.getStore();
    if (!entries) throw new Error("IDM provenance scope is absent");
    if (!entries.some((entry) => entry.kind === kind && entry.behavior === behavior)) {
      entries.push({ kind, behavior, ...(source ? { source } : {}) });
    }
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
