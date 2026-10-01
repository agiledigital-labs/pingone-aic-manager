import type { Case } from "../case/types.ts";
import type { LeaseIdentity } from "./lease-identity.ts";
export interface EmitJourneyOptions {
    /** Short token used in tree/script names. Must match `[A-Za-z0-9-]{1,32}`. */
    runId?: string;
    realm?: string;
    idFactory?: () => string;
}
export interface WrapperInvoke {
    headers: Record<string, string[]>;
    parameters: Record<string, string[]>;
    cookies: Record<string, string>;
}
export interface ScriptResource {
    id: string;
    name: string;
    source: string;
    role: "subject" | "result";
    /** Baked-in outcome, only for `role: "result"`. */
    outcome?: string;
}
export interface NodeResource {
    id: string;
    displayName: string;
    scriptId: string;
    outcomes: string[];
    connections: Record<string, string>;
    x: number;
    y: number;
}
export interface WrapperJourney {
    treeName: string;
    realm: string;
    identityResource: string;
    scripts: ScriptResource[];
    nodes: NodeResource[];
    /** Tree PUT body. No `_id` / `_rev`. */
    treeBody: Record<string, unknown>;
    /** Node PUT bodies keyed by node id. */
    nodeBodies: Record<string, Record<string, unknown>>;
    subjectOutcomes: string[];
    invoke: WrapperInvoke;
}
export interface LeasedJourneyOptions {
    identity: LeaseIdentity;
    realm: string;
    suiteName: string;
}
/**
 * Outcomes declared on the subject node. Always includes `true` and `false`
 * plus `expect.outcome` and everything `case.outcomes` declares, so a script
 * that takes the other branch still reaches a result node that can dump state.
 * A pass expected to suspend has no outcome to add, which is why the declared
 * vocabulary is folded in as well — otherwise a step chain would wire up only
 * the outcomes its first pass mentions.
 */
export declare function subjectOutcomes(kase: Case): string[];
export declare function emitWrapperJourney(kase: Case, source: string, options?: EmitJourneyOptions): WrapperJourney;
/** Emit the fixed graph owned by one file lease. */
export declare function emitLeasedJourney(options: LeasedJourneyOptions): WrapperJourney;
export declare const STATIC_NODE_IDS: {
    readonly success: "70e691a5-1e33-4ac3-a356-e7b6d60d92e0";
    readonly failure: "e301438c-0bd0-429c-ab0c-66126501069a";
};
