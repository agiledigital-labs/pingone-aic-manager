/** Supported Vitest setupFiles entry: per-test ownership; pair with vitest-runner/reporter for the run verdict. */
import { useEmulationGuard } from "./emulation-guard.ts";
import { hasEmulationAuditTransport } from "../emulation.ts";
if (!hasEmulationAuditTransport()) throw new Error("Emulation setup requires the vitest-runner; configure the package vitest-runner and vitest-reporter alongside vitest-setup");
useEmulationGuard();
