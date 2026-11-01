/** Main-to-worker reporter-presence handshake, using Vitest's public context. */
export const EMULATION_REPORTER_CONTEXT = "rhinoLocalEmulationReporter";
export const EMULATION_REPORTER_VERSION = 1;
declare module "vitest" {
  interface ProvidedContext { rhinoLocalEmulationReporter?: number }
}
