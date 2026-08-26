// GENERATED from docs/api/bindings/pingone-verify-completion-next.json by
// scripts/gen-binding-types.mjs — do not edit by hand. Context: PINGONE_VERIFY_COMPLETION_DECISION_NODE.
// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.
// Applied metadata refinements:
//   - Fluent builder methods return their own interface, not metadata's bare `object`.

declare const verifyTransactionsHelper: any;
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

interface Action {
  withIdentifiedUser(username: StringLike): Action;
  withIdentifiedAgent(agentName: StringLike): Action;
  goTo(outcome: StringLike): Action;
  suspend(
    callbackTextFormat: StringLike,
    additionalLogic: object,
    maximumSuspendDuration: number
  ): Action;
  suspend(callbackTextFormat: StringLike): Action;
  suspend(callbackTextFormat: StringLike, additionalLogic: object): Action;
  withHeader(header: StringLike): Action;
  withStage(stage: StringLike): Action;
  putSessionProperty(key: StringLike, value: StringLike): Action;
  withDescription(description: StringLike): Action;
  withErrorMessage(errorMessage: StringLike): Action;
  withLockoutMessage(lockoutMessage: StringLike): Action;
  removeSessionProperty(key: StringLike): Action;
  withMaxSessionTime(maxSessionTime: number): Action;
  withMaxIdleTime(maxIdleTime: number): Action;
}
declare const action: Action;
