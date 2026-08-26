// GENERATED from docs/api/bindings/social-provider-handler-next.json by
// scripts/gen-binding-types.mjs — do not edit by hand. Context: SOCIAL_PROVIDER_HANDLER_NODE.
// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.
// Applied metadata refinements:
//   - Fluent builder methods return their own interface, not metadata's bare `object`.

declare const requestParameters: RequestMap;
declare const normalizedProfile: object;
declare const requestHeaders: RequestMap;
interface NodeState {
  remove(key: StringLike): void;
  get(key: StringLike): object;
  keys(): object;
  isDefined(key: StringLike): boolean;
  getObject(key: StringLike): object;
  putTransient(key: StringLike, value: object): NodeState;
  putShared(key: StringLike, value: object): NodeState;
  mergeShared(object: object): NodeState;
  mergeTransient(object: object): NodeState;
}
declare const nodeState: NodeState;

declare const existingSession: object;
declare const selectedIdp: StringLike;
