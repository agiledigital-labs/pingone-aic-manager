import { beforeEach } from "vitest";
beforeEach(() => { (globalThis as typeof globalThis & { clientSetupRan?: boolean }).clientSetupRan = true; });
