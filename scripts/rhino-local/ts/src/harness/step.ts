import type {
  CallbackEffect,
  Expect,
  Given,
  JsonValue,
  RecordedEffects,
} from "../case/types.ts";
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
export function carryGiven(
  previous: Given,
  effects: RecordedEffects,
  submitted: readonly CallbackEffect[]
): Given {
  const next: Given = { ...previous };
  next.sharedState = clone(effects.sharedState.final);
  // Not `{}`: absent and empty differ elsewhere in `Given`, and "the tenant
  // dropped it" is absence.
  delete next.transientState;
  if (Object.keys(effects.secureState.final).length > 0) {
    next.secureState = clone(effects.secureState.final);
  } else {
    delete next.secureState;
  }
  next.callbacks = submitted.map((callback) => ({ ...callback }));
  if (effects.managedStore !== undefined) {
    // Records the earlier pass created have to be visible to the later one,
    // or a chain can never test a journey that writes and then reads back.
    next.managed = clone(effects.managedStore);
  }
  return next;
}

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
export function submittedCallbacks(
  emitted: readonly CallbackEffect[],
  replies: readonly CallbackReply[],
  label: string
): CallbackEffect[] {
  const pending = new Map<string, JsonValue[]>();
  for (const reply of replies) {
    const queue = pending.get(reply.type);
    if (queue === undefined) {
      pending.set(reply.type, [reply.value]);
    } else {
      queue.push(reply.value);
    }
  }
  const out: CallbackEffect[] = [];
  for (const callback of emitted) {
    const queue = pending.get(callback.type);
    const { value: _outbound, ...rest } = callback;
    if (queue === undefined || queue.length === 0) {
      out.push({ ...rest });
      continue;
    }
    out.push({ ...rest, value: queue.shift() as JsonValue });
  }
  const leftover = [...pending.entries()].filter(([, queue]) => queue.length > 0);
  if (leftover.length > 0) {
    const emittedCounts = countByType(emitted);
    const detail = leftover
      .map(
        ([type, queue]) =>
          `${queue.length} unused ${type} ${queue.length === 1 ? "reply" : "replies"} (the pass emitted ${emittedCounts.get(type) ?? 0})`
      )
      .join("; ");
    throw new Error(
      `rhino-local: ${label} replied to callbacks the pass did not send — ${detail}. A reply nobody asked for would seed the next pass with a value the tenant could never deliver`
    );
  }
  return out;
}

function countByType(callbacks: readonly CallbackEffect[]): Map<string, number> {
  const counts = new Map<string, number>();
  for (const callback of callbacks) {
    counts.set(callback.type, (counts.get(callback.type) ?? 0) + 1);
  }
  return counts;
}

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}
