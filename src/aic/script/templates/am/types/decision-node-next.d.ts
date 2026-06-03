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

// The full set of callback builders present on the live next-gen binding
// (member names verified via enumeration 2026-06-04). Each method queues a
// callback to return to the client. Signatures for the common callbacks follow
// the standard AM callback constructors; the specialized/social ones at the
// bottom are present but their exact argument sets are not yet verified
// (reflection is blocked in the sandbox) — see docs/api/12 open item #4.
interface CallbacksBuilder {
  /** Prompt for a username (`NameCallback`). */
  nameCallback(prompt: StringLike): void;
  /** Prompt for a password; `echoOn` shows typed chars (`PasswordCallback`). */
  passwordCallback(prompt: StringLike, echoOn?: boolean): void;
  /** Free-text input, optionally pre-filled (`TextInputCallback`). */
  textInputCallback(prompt: StringLike): void;
  textInputCallback(prompt: StringLike, defaultText: StringLike): void;
  /**
   * Display a message to the user (`TextOutputCallback`).
   * @param messageType 0 Information · 1 Warning · 2 Error · 4 Script
   */
  textOutputCallback(messageType: 0 | 1 | 2 | 4, message: StringLike): void;
  /** Inject client-side JavaScript (`ScriptTextOutputCallback`). */
  scriptTextOutputCallback(script: string): void;
  /** Round-trip a hidden value with the form (`HiddenValueCallback`). */
  hiddenValueCallback(id: StringLike): void;
  hiddenValueCallback(id: StringLike, value: StringLike): void;
  /**
   * Confirmation buttons (`ConfirmationCallback`). `options` are the button
   * labels and `defaultOption` is the default index.
   * @param messageType 0 Information · 1 Warning · 2 Error
   */
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
  /** Single- or multi-select choice list (`ChoiceCallback`). */
  choiceCallback(
    prompt: StringLike,
    choices: string[],
    defaultChoice: number,
    multipleSelectionsAllowed: boolean
  ): void;
  /** Spinner/poll; `waitTime` is milliseconds as a string (`PollingWaitCallback`). */
  pollingWaitCallback(waitTime: StringLike, message: StringLike): void;
  /** Message shown while the journey is suspended (`SuspendedTextOutputCallback`). */
  suspendedTextOutputCallback(message: StringLike): void;
  /** String attribute input, with optional policy validation. */
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
  /** Numeric attribute input. */
  numberAttributeInputCallback(
    id: string,
    prompt: string,
    value: number | null,
    required: boolean
  ): void;
  /** Boolean attribute input. */
  booleanAttributeInputCallback(
    id: string,
    prompt: string,
    value: boolean | null,
    required: boolean
  ): void;
  /** Knowledge-based-auth question setup (`KbaCreateCallback`). */
  kbaCreateCallback(prompt: StringLike, predefinedQuestions: string[]): void;
  /** Terms & conditions acceptance (`TermsAndConditionsCallback`). */
  termsAndConditionsCallback(
    version: StringLike,
    terms: StringLike,
    createDate: StringLike
  ): void;
  /** Collect a device profile (metadata and/or geolocation). */
  deviceProfileCallback(
    metadata: boolean,
    location: boolean,
    message: StringLike
  ): void;

  // Present on the binding (verified), but exact argument sets are NOT yet
  // verified — typed permissively so they autocomplete without false errors.
  // Tighten via docs/api/12 open item #4.
  /** @remarks Signature not yet verified (docs/api/12 #4). */
  validatedUsernameCallback(...args: any[]): void;
  /** @remarks Signature not yet verified (docs/api/12 #4). */
  validatedPasswordCallback(...args: any[]): void;
  /** @remarks Signature not yet verified (docs/api/12 #4). */
  redirectCallback(...args: any[]): void;
  /** @remarks Signature not yet verified (docs/api/12 #4). */
  metadataCallback(...args: any[]): void;
  /** @remarks Signature not yet verified (docs/api/12 #4). */
  selectIdPCallback(...args: any[]): void;
  /** @remarks Signature not yet verified (docs/api/12 #4). */
  idPCallback(...args: any[]): void;
  /** @remarks Signature not yet verified (docs/api/12 #4). */
  consentMappingCallback(...args: any[]): void;
  /** @remarks Signature not yet verified (docs/api/12 #4). */
  languageCallback(...args: any[]): void;
  /** @remarks Signature not yet verified (docs/api/12 #4). */
  x509CertificateCallback(...args: any[]): void;
  /** @remarks Signature not yet verified (docs/api/12 #4). */
  httpCallback(...args: any[]): void;
}
declare const callbacksBuilder: CallbacksBuilder;

interface RequestCookies {
  get: (key: StringLike) => JavaArray<JavaString> | null;
  containsKey: (key: StringLike) => boolean;
}
declare const requestCookies: RequestCookies;
