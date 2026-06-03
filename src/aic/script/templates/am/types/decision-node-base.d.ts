// Scripted-decision bindings shared by BOTH engine generations. Layered on top
// of rhino-1.7.14.d.ts + common.d.ts. The next-gen and legacy overlays add only
// the globals unique to each generation.
//
// Shapes target the next-generation engine (the primary edit target). Whether
// legacy (evaluatorVersion 1.0) exposes identical object shapes is a deferred
// probe (matrix open item #1).

declare function _nodeStateGet(
  key: StringLike
): Record<string, any> | JavaString | boolean | null | undefined;
declare function _nodeStateGet(
  key: "objectAttributes"
): Record<string, any> | null | undefined;

interface NodeState {
  get: typeof _nodeStateGet;
  getObject: (key: StringLike) => Object | null | undefined;
  putShared: (key: StringLike, value: any) => void;
  putTransient: (key: StringLike, value: any) => void;
}
declare const nodeState: NodeState;

interface RequestParameters {
  get: (key: StringLike) => JavaArray<JavaString> | null;
}
declare const requestParameters: RequestParameters;

interface RequestHeaders {
  get: (key: StringLike) => JavaArray<JavaString> | null;
  containsKey: (key: StringLike) => boolean;
}
declare const requestHeaders: RequestHeaders;

interface Callbacks {
  getTextInputCallbacks: () => JavaArray<string>;
  getHiddenValueCallbacks: () => JavaArray<string>;
  getStringAttributeInputCallbacks: () => JavaArray<string>;
  getConfirmationCallbacks: () => JavaArray<number>;
  isEmpty(): boolean;
}
declare const callbacks: Callbacks;

interface Identity {
  getAttributeValues: (attributeName: string) => JavaArray<string>;
  setAttribute: (attributeName: string, value: [string] | []) => void;
  store: () => void;
}
interface IdRepository {
  getIdentity: (id: string) => Identity;
}
declare const idRepository: IdRepository;

interface Action {
  goTo: (outcome: StringLike) => Action;
  withHeader: (header: StringLike) => Action;
  withDescription: (html: StringLike) => Action;
  withStage: (stage: StringLike) => Action;
  putSessionProperty: (sessionKey: StringLike, value: any) => Action;
}
declare const action: Action;

// Legacy way to set the outcome (next-gen prefers action.goTo). Still works.
declare let outcome: StringLike | undefined;

interface ExistingSession {
  Principal: string;
}
declare const existingSession: ExistingSession | undefined;
