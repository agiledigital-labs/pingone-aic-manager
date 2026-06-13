// Scripted-decision bindings shared by BOTH engine generations. Layered on top
// of rhino-1.7.14.d.ts + common.d.ts. The next-gen and legacy overlays add only
// the globals unique to each generation.
//
// nodeState/secrets shapes are transcribed from the next-gen binding metadata
// (docs/api/bindings/scripted-decision-next.json) and confirmed present on the
// legacy engine too (probe 2026-06-04). On legacy, nodeState.get() returns a
// Java JsonValue needing .asString()/.asMap(); on next-gen it returns a coerced
// JS value — a return-shape difference, not a presence one.

declare function _nodeStateGet(
  key: StringLike
): Record<string, any> | JavaString | boolean | null | undefined;
declare function _nodeStateGet(
  key: "objectAttributes"
): Record<string, any> | null | undefined;

interface NodeState {
  get: typeof _nodeStateGet;
  getObject: (key: StringLike) => object | null | undefined;
  /** True if the key is set in any state. */
  isDefined: (key: StringLike) => boolean;
  /** All defined state keys. */
  keys: () => object;
  /** Remove a key from shared state. */
  remove: (key: StringLike) => void;
  putShared: (key: StringLike, value: any) => NodeState;
  putTransient: (key: StringLike, value: any) => NodeState;
  mergeShared: (object: object) => NodeState;
  mergeTransient: (object: object) => NodeState;
}
declare const nodeState: NodeState;

declare const requestParameters: RequestMap;
declare const requestHeaders: RequestMap;

// Accessors for callbacks returned from a previous pass through this node.
interface Callbacks {
  isEmpty(): boolean;
  getNameCallbacks(): JavaArray<string>;
  getPasswordCallbacks(): JavaArray<string>;
  getHiddenValueCallbacks(): object;
  getChoiceCallbacks(): JavaArray<string>;
  getConfirmationCallbacks(): JavaArray<number>;
  getTextInputCallbacks(): JavaArray<string>;
  getStringAttributeInputCallbacks(): JavaArray<string>;
  getNumberAttributeInputCallbacks(): JavaArray<string>;
  getBooleanAttributeInputCallbacks(): JavaArray<string>;
  getDeviceProfileCallbacks(): JavaArray<string>;
  getKbaCreateCallbacks(): JavaArray<string>;
  getSelectIdPCallbacks(): JavaArray<string>;
  getTermsAndConditionsCallbacks(): JavaArray<string>;
  getLanguageCallbacks(): JavaArray<string>;
  getIdpCallbacks(): JavaArray<string>;
  getValidatedUsernameCallbacks(): JavaArray<string>;
  getValidatedPasswordCallbacks(): JavaArray<string>;
  getHttpCallbacks(): JavaArray<string>;
  getX509CertificateCallbacks(): JavaArray<string>;
  getConsentMappingCallbacks(): JavaArray<string>;
}
declare const callbacks: Callbacks;

interface Identity {
  // Method syntax (not an arrow property) so next-gen can merge a typed
  // attribute-name overload onto it (decision-node-next.d.ts + AmUserAttribute).
  getAttributeValues(attributeName: string): JavaArray<string>;
  setAttribute: (attributeName: string, value: [string] | []) => void;
  store: () => void;
}
interface IdRepository {
  getIdentity: (userName: string) => Identity;
}
declare const idRepository: IdRepository;

// Both engines let you set the journey outcome by assigning `outcome`
// (next-gen also offers the `action` binding — see decision-node-next.d.ts).
declare let outcome: StringLike | undefined;

interface ExistingSession {
  Principal: string;
}
declare const existingSession: ExistingSession | undefined;

// Present on both engines (verified 2026-06-04).
declare const resumedFromSuspend: boolean;
