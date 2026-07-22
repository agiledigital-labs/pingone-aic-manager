// Library script bindings. Library scripts are next-generation CommonJS modules:
// they have the common next-gen bindings (see common.d.ts) PLUS the CommonJS
// module mechanics below. Only next-generation scripts can require() libraries,
// and libraries may require other libraries (verified by corpus usage —
// 17 sandbox libraries require others).
//
// Libraries receive per-call state (e.g. nodeState) as function arguments via
// the common `.load(...)` factory pattern, so the scripted-decision globals are
// intentionally NOT in scope here. (`require` comes from nextgen-common.d.ts.)

// The scripted-decision base defs are excluded to keep their globals out of
// library scope, so redeclare the argument types that library factories need.
interface NodeState {
  get(key: "objectAttributes"): Record<string, any> | null | undefined;
  get(key: StringLike): Record<string, any> | JavaString | boolean | null | undefined;
  getObject(key: StringLike): object | null | undefined;
  /** True if the key is set in any state. */
  isDefined(key: StringLike): boolean;
  /** All defined state keys. */
  keys(): object;
  /** Remove a key from shared state. */
  remove(key: StringLike): void;
  putShared(key: StringLike, value: any): NodeState;
  putTransient(key: StringLike, value: any): NodeState;
  mergeShared(object: object): NodeState;
  mergeTransient(object: object): NodeState;
}

type RequestHeaders = RequestMap;
type RequestParameters = RequestMap;

declare const module: { exports: any };
declare const exports: any;
