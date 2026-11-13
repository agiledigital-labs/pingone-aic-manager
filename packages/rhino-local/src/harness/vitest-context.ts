/** Main-to-worker reporter-presence handshake, using Vitest's public context. */
import type { PoolAddress } from "../pool.ts";
export const EMULATION_REPORTER_CONTEXT = "rhinoLocalEmulationReporter";
export const EMULATION_REPORTER_VERSION = 1;
declare module "vitest" {
  interface ProvidedContext {
    rhinoLocalEmulationReporter?: number;
    /** Where the reporter's JVM pool listens; absent when the pool is off. */
    rhinoLocalJvmPool?: PoolAddress;
  }
}
