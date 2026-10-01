import type { CallbackEffect, JsonValue } from "../case/types.ts";
export interface AuthenticateCallbacks {
    /** JSON payload from the result node's HiddenValueCallback, if present. */
    dumpRaw: string | undefined;
    /** Subject-node callbacks; harness dump stripped. */
    callbacks: CallbackEffect[];
}
/**
 * Read `/authenticate` callbacks. `type` is taken from the HTTP field as-is
 * (the effects-contract spelling: Java simple class name, PascalCase).
 * Other fields are the callback's `output` name/value pairs, so a NameCallback
 * becomes `{ type: "NameCallback", prompt: "User Name" }` — no `input` (that
 * is the client's reply, not the effect).
 */
export declare function parseAuthenticateCallbacks(body: unknown): AuthenticateCallbacks;
/**
 * Build the body that answers a pass's callbacks.
 *
 * AM wants the whole `/authenticate` response back with each callback's
 * `input` slot filled, so the response is cloned and edited rather than
 * rebuilt — `authId` and every field the harness does not understand survive
 * untouched. Replies are matched to callbacks by type, in order, exactly as
 * the local lane's `submittedCallbacks` matches them; a reply the pass never
 * asked for is refused rather than dropped, because a chain that silently
 * ignores a reply is a test asserting something it never sent.
 *
 * Callbacks with no reply keep whatever AM put in their input slot. That is
 * the tenant's own default, which is the one case where a default is not a
 * guess (an unanswered HiddenValueCallback comes back holding its `id`,
 * measured 2026-09-14).
 */
export declare function fillCallbackInputs(body: unknown, replies: readonly {
    type: string;
    value: JsonValue;
}[], label: string): string;
