/** Minimal setup dependency graph: do not preload application modules before vi.mock. */
import { aroundAll, aroundEach } from "vitest";
import { createEmulationOwner, currentEmulationOwner, expectOwnedEmulationFailure,
  verifyEmulationOwner, withEmulationOwner, type EmulationFailureSource } from "../emulation.ts";

export type EmulationGuardSource = EmulationFailureSource | (() => EmulationFailureSource | undefined);

/** Global ownership guard. Source arguments are retained for source compatibility;
 * the process registry covers all sources regardless of fixture replacement. */
export function useEmulationGuard(..._inputs: EmulationGuardSource[]): void {
  // eslint-disable-next-line no-empty-pattern
  aroundAll(async (runSuite, {}, scope) => {
    if (currentEmulationOwner()) return runSuite();
    const owner = createEmulationOwner(scope.name, "suite");
    await withEmulationOwner(owner, async () => {
      try { await runSuite(); } finally { verifyEmulationOwner(owner); }
    });
  });
  aroundEach(async (runTest, { task }) => {
    // Global setup and useLease/legacy helpers can coexist without nested owners.
    if (currentEmulationOwner()?.kind === "test") return runTest();
    const owner = createEmulationOwner(task.name, "test", task.id);
    await withEmulationOwner(owner, async () => {
      try { await runTest(); } finally { verifyEmulationOwner(owner); }
    });
  });
}

export function expectEmulationFailure(name: string | RegExp): void { expectOwnedEmulationFailure(name); }

