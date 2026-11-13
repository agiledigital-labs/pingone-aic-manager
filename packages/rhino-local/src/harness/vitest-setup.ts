/** Supported Vitest setupFiles entry: per-test ownership; pair with vitest-runner/reporter for the run verdict. */
import { inject } from "vitest";
import { EMULATION_REPORTER_CONTEXT, EMULATION_REPORTER_VERSION } from "./vitest-context.ts";
import { useEmulationGuard } from "./emulation-guard.ts";
import { hasEmulationAuditTransport } from "../emulation.ts";
if (!hasEmulationAuditTransport()) throw new Error("Emulation setup requires the vitest-runner; configure the package vitest-runner and vitest-reporter alongside vitest-setup");
if (inject(EMULATION_REPORTER_CONTEXT) !== EMULATION_REPORTER_VERSION) throw new Error("Emulation setup requires the vitest-reporter; retain the package vitest-reporter alongside other reporters (CLI --reporter replaces configured reporters)");
useEmulationGuard();
