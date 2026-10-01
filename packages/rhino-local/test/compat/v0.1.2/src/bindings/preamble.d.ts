import type { Given } from "../case/types.ts";
import type { EnvProfile } from "../profile/types.ts";
export interface MockPreambleOptions {
    /**
     * Optional override for direct runner callers. Suite cases carry libraries
     * in `given` and need no separate runner option.
     */
    libraries?: Record<string, string>;
    /**
     * Pulled environment schema. Only the set of declared object types crosses
     * into the sandbox — every schema rule is enforced in the Node layer, so
     * there is one implementation of each rather than an AM-safe copy.
     */
    profile?: EnvProfile;
}
/**
 * Generated stubs + behaviour overlay + seed. Eval'd as the runner `preamble`
 * so author line numbers on `source` stay intact.
 */
export declare function mockPreamble(given?: Given, options?: MockPreambleOptions): string;
/** Append a harvest call without shifting author line numbers. */
export declare function withHarvest(script: string): string;
