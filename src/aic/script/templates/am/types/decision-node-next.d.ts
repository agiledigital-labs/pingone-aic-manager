// Next-generation-only scripted-decision bindings (evaluatorVersion 2.0).
// Layered on top of rhino + common + decision-node-base.
//
// Verified present in a next-gen scripted decision node (2026-06-03):
//   callbacksBuilder, requestCookies, resumedFromSuspend, secrets.
// (openidm/utils/httpClient are common to all next-gen scripts and live in
// common.d.ts.)

// Next-gen scripted decision scripts can require() library scripts (resolved via
// the leaf tsconfig `paths` alias to ../lib/*).
declare function require(id: string): any;

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

// True when the journey resumed after action.suspend().
declare const resumedFromSuspend: boolean;

// `secrets` is verified present, but its method shape is not yet probed; this is
// the documented AM SecretsApi shape (treat as provisional — matrix open #2).
interface Secrets {
  getGenericSecret(name: StringLike): JavaString;
}
declare const secrets: Secrets;
