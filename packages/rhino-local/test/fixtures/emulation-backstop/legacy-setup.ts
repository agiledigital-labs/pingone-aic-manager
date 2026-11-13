// Deliberately omit supported setup's configuration check to exercise the
// worker's failed-task defence independently of reporter-presence refusal.
import { useEmulationGuard } from "../../../src/harness/emulation-guard.ts";
useEmulationGuard();
