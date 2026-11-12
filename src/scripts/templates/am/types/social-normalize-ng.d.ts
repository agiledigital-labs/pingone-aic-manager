// GENERATED from docs/api/bindings/social-idp-profile-transformation-next.json by
// scripts/gen-binding-types.mjs — do not edit by hand. Context: SOCIAL_IDP_PROFILE_TRANSFORMATION_NEXT_GEN.
// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.
// Applied metadata refinements:
//   - Fluent builder methods return their own interface, not metadata's bare `object`.
//   - `rawProfile` is a `RawProfile`; `nodeState.get` returns `any`, `put*` take `any`, and `keys` is not declared.
//   - NodeState members other than `get`, `putShared`, `putTransient` are the descriptor's, unmeasured here.

// The provider's profile: a Java map whose values read back as JS values. `.sub` and
// `get("sub")` are JS strings, `get(missing)` is `null`, `for...in` lists the keys.
// `getObject` and `isDefined` are not functions (measured 2026-11-10, docs/api/12).
// The base tsconfig forbids dot access to an index-signature key, so write
// `rawProfile["sub"]` or `rawProfile.get("sub")`.
interface RawProfile {
  get(key: StringLike): unknown;
  keys(): MeasuredJavaList<unknown>;
  size(): number;
  [key: string]: unknown;
}

declare const requestParameters: RequestMap;
interface EmailService {
  send(to: StringLike, subject: StringLike, body: StringLike): void;
  send(
    to: StringLike,
    subject: StringLike,
    body: StringLike,
    mimeType: StringLike
  ): void;
}
declare const emailService: EmailService;

declare const requestHeaders: RequestMap;
declare const rawProfile: RawProfile;
interface NodeState {
  remove(key: StringLike): void;
  get(key: StringLike): any;
  isDefined(key: StringLike): boolean;
  getObject(key: StringLike): object;
  putShared(key: StringLike, value: any): NodeState;
  mergeShared(object: object): NodeState;
  mergeTransient(object: object): NodeState;
  putTransient(key: StringLike, value: any): NodeState;
}
declare const nodeState: NodeState;

// `existingSession` is listed by the descriptor but is a ReferenceError on a request
// with no session cookie (measured 2026-11-10); with one it is unmeasured here.
// Guard with `typeof existingSession !== "undefined"` before reading it.
declare const existingSession: object | undefined;
declare const selectedIdp: StringLike;

// The result is the script's COMPLETION VALUE: a plain object (`{}` too) is handed to
// the handler; `undefined`, `null` or a string fails the journey (401). Numbers become
// doubles, and an `undefined` top-level property arrives as `null`.
