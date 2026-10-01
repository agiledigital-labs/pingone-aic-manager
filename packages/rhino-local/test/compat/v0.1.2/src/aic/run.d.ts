import type { TenantProvider } from "./provider.ts";
import type { Case, JsonValue, RecordedEffects } from "../case/types.ts";
import { type WrapperJourney } from "./emit-journey.ts";
import { type ManagedFixture } from "./managed.ts";
import { type AicIo, type AmResponse, type TenantSession } from "./tenant.ts";
/** One submitted callback value, matched to an emitted callback by type. */
export interface AicReply {
    type: string;
    value: JsonValue;
}
export interface RunAicOptions {
    io?: AicIo;
    tenant?: string;
    realm?: string;
    runId?: string;
    project?: string;
    /** Where the tenant and its bearer come from; see `resolveTenantProvider`. */
    provider?: TenantProvider;
    /** Harness-owned records from the local lease's fixture ledger. */
    managedFixtures?: readonly ManagedFixture[];
    /**
     * One entry per suspended pass, in order: what the client submits to advance
     * the journey. Omit for a single-pass case. The journey is provisioned once
     * and every pass re-enters the same node, which is what makes this the
     * counterpart of the local lane's `.step()` chain rather than N runs.
     */
    replies?: readonly (readonly AicReply[])[];
}
export interface AicRunValidation {
    harnessOwnsManaged: boolean;
    unsupported: string[];
}
export type AicPassObserver = (index: number, effects: RecordedEffects) => void | Promise<void>;
export type CreatedResource = {
    kind: "script";
    id: string;
} | {
    kind: "node";
    id: string;
} | {
    kind: "tree";
    name: string;
};
/**
 * Create a namespaced wrapper journey, invoke it once, record effects, and
 * delete everything created. Refuses to overwrite an existing script, node,
 * or tree. Every `aic` invocation goes through `--no-prompt`.
 */
export declare function runAicLane(kase: Case, source: string, options?: RunAicOptions): Promise<RecordedEffects>;
/**
 * One-shot compatibility facade: run a step chain on a tenant, provisioning
 * and deleting one throwaway journey around this call. `useLease()` does not
 * call this function; it hands the recorded chain to its pre-opened
 * `AicFileLease` so stable resources are reused across tests.
 *
 * `cases` is one `Case` per pass — `cases[0].given` seeds the journey and every
 * later `given` is what the local lane computed for that pass. Handing the
 * carried seeds over rather than recomputing them here is the whole point of
 * the two lanes: the tenant's own `before` snapshot is checked against them
 * (`verifySeedsVisible`), so a carry rule that is wrong — carrying transient
 * state across a suspend, say — fails loudly here instead of agreeing with
 * itself locally.
 *
 * Managed state is accepted only with a fixture ledger that exactly matches
 * the first pass. Those records are create-only seeded, read back, and removed
 * around the whole journey while a per-tenant lock excludes parallel fixture
 * runs. Later passes may carry managed changes made by the subject itself.
 *
 * `options.replies` must hold one entry per suspended pass, so
 * `replies.length === cases.length - 1`.
 */
export declare function runAicChain(cases: readonly Case[], source: string, options?: RunAicOptions): Promise<RecordedEffects[]>;
/**
 * Shared fail-closed validation for the throwaway and reusable runners.
 * Lease-specific source, realm, and outcome constraints remain with the
 * lease because the one-shot facade has no pre-opened graph to compare.
 */
export declare function validateAicRun(cases: readonly Case[], replies: readonly (readonly AicReply[])[], managedFixtures?: readonly ManagedFixture[]): AicRunValidation;
/**
 * The session cookie's name is per-tenant, so it has to be read rather than
 * assumed (`given.cookieName` is refused on this lane for the same reason).
 */
export declare function fetchCookieName(io: AicIo, session: TenantSession): Promise<string>;
export declare function provisionJourney(io: AicIo, session: TenantSession, wrapper: WrapperJourney, created: CreatedResource[], beforeWrite?: () => void): Promise<void>;
/**
 * Invoke the journey, then answer each declared pass in turn.
 *
 * Every pass re-enters the same node, so the tenant carries state between them
 * on its own terms — which is the point: the local lane has to model that, and
 * this is what it is modelled against. A pass that reaches the result node
 * while replies are still pending is an error, because the chain then asserts
 * against a journey shorter than it described.
 */
export declare function driveJourney(io: AicIo, session: TenantSession, wrapper: WrapperJourney, cases: readonly Case[], replies: readonly (readonly AicReply[])[], tx: {
    next: () => string;
}, proof?: {
    leaseDigest: string;
    invocationNonce: string;
    subjectDigest: string;
}, observePass?: AicPassObserver): Promise<RecordedEffects[]>;
export declare function invokeJourney(io: AicIo, session: TenantSession, wrapper: WrapperJourney, transactionId: string, body?: string): Promise<AmResponse>;
export declare function deleteCreatedResources(io: AicIo, session: TenantSession, realm: string, created: readonly CreatedResource[]): Promise<string[]>;
