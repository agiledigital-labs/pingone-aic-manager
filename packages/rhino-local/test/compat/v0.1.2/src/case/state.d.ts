import type { JsonObject, StateMutation } from "./types.ts";
/** Net JSON mutations between two views of one state map. */
export declare function diffState(initial: JsonObject, final: JsonObject): StateMutation[];
export declare function sameMutationValue(a: StateMutation, b: StateMutation): boolean;
