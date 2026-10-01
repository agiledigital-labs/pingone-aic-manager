import type { CallbackEffect, Expect, Given, JsonValue, RecordedEffects } from "../case/types.ts";
import type { IdmHandle } from "./types.ts";
/**
 * One submitted callback value — what the client types into the callback the
 * script sent. `type` names the callback it answers; the harness matches
 * replies to emitted callbacks by type, in order.
 */
export interface CallbackReply {
    type: string;
    value: JsonValue;
}
export interface StepContext<TInput> {
    input: TInput;
    /** 1-based position in the chain. */
    step: number;
    /** What this pass emitted, in order. */
    callbacks: CallbackEffect[];
    effects: RecordedEffects;
}
/** Everything an intermediate pass can assert, minus the outcome it cannot have. */
export type StepExpect = Omit<Expect, "outcome">;
export interface StepSpec<TInput> {
    /**
     * What this pass must do before it suspends. `outcome` is not offered: a
     * pass that decided an outcome did not suspend, and the chain has nothing
     * to reply to.
     */
    expect?: StepExpect;
    /**
     * What the client submits back. A function receives what the pass emitted,
     * which is how a reply depends on the choices the script offered.
     */
    reply: CallbackReply[] | ((ctx: StepContext<TInput>) => CallbackReply[]);
    /**
     * Asserted on each lane after this pass and before the reply is submitted —
     * the point of the chain is to check the world between two halves of a
     * journey, not only at the end. AIC cannot dump intermediate node state
     * without adding a script-visible callback, so `effects.evidence` marks
     * those channels unobserved there. Fail by throwing.
     */
    check?: (idm: IdmHandle, ctx: StepContext<TInput>) => void | Promise<void>;
}
/**
 * Seed the next pass from the pass that just suspended.
 *
 * MEASURED 2026-09-14 against a live tenant, one scripted decision node that
 * put a value in each bucket, sent a NameCallback, and read them back on the
 * resumed pass (`docs/api/09-journeys.md` → "What survives a callback round
 * trip"):
 *
 * - **shared state survives.** `nodeState.get` returned it and `nodeState.keys()`
 *   still listed it.
 * - **transient state does NOT.** It came back `null`, identical to a key
 *   nobody ever set (the control), and it was absent from `keys()`. It is
 *   dropped — not promoted into secure state, which is the plausible guess a
 *   hand-rolled mock makes and the one that turns a broken script green.
 * - **`resumedFromSuspend` stays false.** It belongs to `action.suspend()`, so
 *   a callback round trip must not set it; seeding `true` here would put the
 *   local lane in a state the AIC lane cannot even reach (the AIC lane refuses
 *   `given.resumedFromSuspend` for exactly that reason).
 *
 * Secure state is carried as the previous pass left it. No next-gen script can
 * write it — there is no `putSecure` — so this is local-lane bookkeeping for a
 * seed the case supplied, not a claim about AM. The AIC lane skips any case
 * that declares `given.secureState`.
 */
export declare function carryGiven(previous: Given, effects: RecordedEffects, submitted: readonly CallbackEffect[]): Given;
/**
 * Merge replies into the callbacks a pass emitted, producing what the client
 * submits.
 *
 * An emitted callback with no reply is submitted **without a value**, so a
 * script that reads it fails naming the type rather than receiving whatever
 * the callback happened to carry outbound. That asymmetry is deliberate: AM
 * fills an unanswered input with a per-type default (an unanswered
 * HiddenValueCallback comes back holding its own `id`, measured 2026-09-14),
 * and inventing those defaults for every callback type would be a mock the
 * tenant does not match.
 */
export declare function submittedCallbacks(emitted: readonly CallbackEffect[], replies: readonly CallbackReply[], label: string): CallbackEffect[];
