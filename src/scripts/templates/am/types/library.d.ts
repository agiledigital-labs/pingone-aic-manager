// Library script bindings. Library scripts are next-generation CommonJS modules:
// they have the common next-gen bindings (see common.d.ts) PLUS the CommonJS
// module mechanics below. Only next-generation scripts can require() libraries,
// and libraries may require other libraries (verified by corpus usage —
// 17 sandbox libraries require others).
//
// Libraries receive per-call state (e.g. nodeState) as function arguments via
// the common `.load(...)` factory pattern, so the scripted-decision globals are
// intentionally NOT in scope here. (`require` comes from nextgen-common.d.ts.)
//
// The globals are out of scope; their TYPES are not, or a factory signature
// could not be written at all. **library-args.d.ts** carries one declaration per
// binding, generated from every next-gen context's metadata — `CallbacksBuilder`,
// `Action`, `Callbacks`, `AccessToken`, and the rest. This file holds only the
// few a caller can pass that the metadata cannot describe.

// The scripted-decision base defs are excluded to keep their globals out of
// library scope, so redeclare the argument types that library factories need.
// `NodeState` is hand-written rather than generated (it is on the `--skip` list
// in library-args.d.ts's regenerate command): the metadata types `get` as a bare
// `object`, which loses the objectAttributes overload and every useful return.
interface NodeState {
  get(key: "objectAttributes"): Record<string, any> | null | undefined;
  get(
    key: StringLike
  ): Record<string, any> | JavaString | boolean | null | undefined;
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

// `existingSession` is an `object` with no enumerated members in the metadata,
// so the generated set cannot name it. Same shape as decision-node-base.d.ts.
interface ExistingSession {
  Principal: string;
}

// Next-gen scripted decision spells this `OAuthApplication`; the metadata name
// pascal-cases to `OauthApplication`. Alias so a factory signature can use the
// spelling the calling script uses.
type OAuthApplication = OauthApplication;

type RequestHeaders = RequestMap;
type RequestParameters = RequestMap;

declare const module: { exports: any };
declare const exports: any;
