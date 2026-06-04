// GENERATED from docs/api/bindings/pingone-verify-completion-next.json by
// scripts/gen-binding-types.mjs — do not edit by hand. Context: PINGONE_VERIFY_COMPLETION_DECISION_NODE.
// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.

declare const verifyTransactionsHelper: any;
interface NodeState {
  remove(key: StringLike): void;
  get(key: StringLike): object;
  keys(): object;
  isDefined(key: StringLike): boolean;
  getObject(key: StringLike): object;
  putTransient(key: StringLike, value: object): object;
  putShared(key: StringLike, value: object): object;
  mergeShared(object: object): object;
  mergeTransient(object: object): object;
}
declare const nodeState: NodeState;

interface Action {
  withIdentifiedUser(username: StringLike): object;
  withIdentifiedAgent(agentName: StringLike): object;
  goTo(outcome: StringLike): object;
  suspend(callbackTextFormat: StringLike, additionalLogic: object, maximumSuspendDuration: number): object;
  suspend(callbackTextFormat: StringLike): object;
  suspend(callbackTextFormat: StringLike, additionalLogic: object): object;
  withHeader(header: StringLike): object;
  withStage(stage: StringLike): object;
  putSessionProperty(key: StringLike, value: StringLike): object;
  withDescription(description: StringLike): object;
  withErrorMessage(errorMessage: StringLike): object;
  withLockoutMessage(lockoutMessage: StringLike): object;
  removeSessionProperty(key: StringLike): object;
  withMaxSessionTime(maxSessionTime: number): object;
  withMaxIdleTime(maxIdleTime: number): object;
}
declare const action: Action;

