// Next-generation-only scripted-decision bindings (evaluatorVersion 2.0).
// Layered on top of rhino + common + nextgen-common + decision-node-base.
//
// Verified next-gen-only (absent on the legacy engine, 2026-06-03):
//   action, callbacksBuilder, requestCookies, require.
// (openidm/utils live in nextgen-common.d.ts; resumedFromSuspend/secrets are in
// both engines and live in decision-node-base.d.ts.)

// Next-gen scripted decision scripts can require() library scripts (resolved via
// the leaf tsconfig `paths` alias to ../lib/*).
declare function require(id: string): any;

// Next-gen `action` binding (legacy imports the Action class via JavaImporter
// instead — verified: no `action` binding on the legacy engine).
interface Action {
  goTo: (outcome: StringLike) => Action;
  withHeader: (header: StringLike) => Action;
  withDescription: (html: StringLike) => Action;
  withStage: (stage: StringLike) => Action;
  putSessionProperty: (sessionKey: StringLike, value: any) => Action;
}
declare const action: Action;

type StringAttributePolicy =
  | {
      policyId: "minimum-length";
      params: { minLength: number };
      policyRequirements: ["MIN_LENGTH"];
    }
  | {
      policyId: "valid-date";
      params: {};
      policyRequirements: ["VALID_DATE"];
    };
type StringAttributePolicyRequirements =
  StringAttributePolicy["policyRequirements"][0];
type StringAttributePolicies = {
  policies: Array<StringAttributePolicy>;
  policyRequirements: Array<StringAttributePolicyRequirements>;
};

interface CallbacksBuilder {
  textInputCallback(message: StringLike): void;
  confirmationCallback(
    messageType: number,
    options: string[],
    defaultOption: number
  ): void;
  confirmationCallback(
    prompt: StringLike,
    messageType: number,
    options: string[],
    defaultOption: number
  ): void;
  /**
   * @param messageType - Type of message:
   * - 0: Information
   * - 1: Warning
   * - 2: Error
   * - 3: Unknown
   * - 4: Script
   */
  textOutputCallback(messageType: 0 | 1 | 2 | 4, message: StringLike): void;
  hiddenValueCallback(id: StringLike, value: StringLike): void;
  scriptTextOutputCallback(js: string): void;
  stringAttributeInputCallback(
    id: string,
    prompt: string,
    value: string | null,
    required: boolean
  ): void;
  stringAttributeInputCallback(
    id: string,
    prompt: string,
    value: string | null,
    required: boolean,
    policy: StringAttributePolicies | undefined,
    evaluatePolicy: boolean,
    failedPolicies: string[]
  ): void;
}
declare const callbacksBuilder: CallbacksBuilder;

interface RequestCookies {
  get: (key: StringLike) => JavaArray<JavaString> | null;
  containsKey: (key: StringLike) => boolean;
}
declare const requestCookies: RequestCookies;
