import type { Given } from "./types.ts";
/**
 * A case is portable when it declares none of the environment-dependent
 * inputs (`esv`, `secrets`, `managed`, `http`). Empty maps/arrays do not
 * count as a declaration. This is a property of the case, not a guarantee
 * that local and AIC verdicts will agree — see the done-note on leaks.
 */
export declare function isPortable(kase: {
    given?: Given;
}): boolean;
