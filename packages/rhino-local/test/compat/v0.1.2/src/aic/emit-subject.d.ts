import type { Given, JsonObject } from "../case/types.ts";
export interface InstrumentedSubject {
    source: string;
    bindingName: string;
    snapshotKey: string;
    subjectDigest?: string;
}
export interface SubjectLeaseProof {
    snapshotKey: string;
    invocationNonce: string;
    header?: string;
}
/**
 * Seed `nodeState`, then capture it immediately before and after the author
 * source.
 *
 * The author source stays at top level: wrapping it in a function or block
 * changes AM's already-unusual top-level `const` behaviour. One generated
 * binding is therefore visible to global enumeration. Its identifier and the
 * state key written after the author source are chosen not to occur anywhere
 * in the original source; helper names are private inside the binding's IIFE.
 *
 * The seed lives here rather than in a preceding setup node, and rather than
 * in a query parameter. Measured 2026-09-13: state seeded in the subject, in
 * a preceding node, and with the subject as the entry node all produced
 * byte-identical results, so the extra node bought nothing. A query parameter
 * would have been worse than useless — `requestParameters` is a binding the
 * corpus measures, so harness data sitting in it is observable to the very
 * scripts under test.
 *
 * The seed is applied BEFORE the `before` snapshot, matching what the setup
 * node did: seeded values are the script's starting state, not a mutation it
 * made. It is applied only on the node's FIRST pass — see the guard below.
 */
export declare function instrumentSubject(authorSource: string, runId: string, given?: Given, proof?: SubjectLeaseProof): InstrumentedSubject;
/** JSON objects the subject will seed, for tests and invoke classification. */
export declare function seededState(given: Given): {
    shared: JsonObject;
    transient: JsonObject;
};
