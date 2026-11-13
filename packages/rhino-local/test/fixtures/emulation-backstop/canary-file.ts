import { expect, it } from "vitest";
import { evidence, recordFrameworkImport } from "./canary-state.ts";
export function registerCanaryFile(): void {
  const file = recordFrameworkImport("test");
  it("framework imports used this file's readiness", ({ task }) => {
    const observed = evidence(file);
    expect(observed.helper).toBe("initialized");
    expect(observed.helperImports).toBeGreaterThan(0);
    (task.meta as { readinessCanary?: typeof observed }).readinessCanary = observed;
  });
}
