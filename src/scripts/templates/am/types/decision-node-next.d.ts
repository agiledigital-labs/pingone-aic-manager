// Next-generation-only scripted-decision bindings (evaluatorVersion 2.0).
// Layered on top of rhino + common + nextgen-common + decision-node-base.
//
// Signatures transcribed from the script editor's binding metadata
// (docs/api/bindings/scripted-decision-next.json, 2026-06-04) — authoritative.
// All verified ABSENT on the legacy engine (2026-06-03).

// (require lives in nextgen-common.d.ts — shared by all next-gen contexts.)

// Typed managed-user attribute names on the scripted-decision identity getter.
// Merged onto the `Identity` interface from decision-node-base.d.ts; declared
// here (next-gen only) so the legacy leaf — which lacks nextgen-common and so
// `AmUserAttribute` — is unaffected. The literal-union overload gives the editor
// autocomplete on AM attribute names (docs/api/14); the `string` signature in
// the base keeps arbitrary names working. NOTE: `idRepository.getIdentity()`
// resolves by managed-object UUID (fr-idm-uuid), NOT userName (verified).
interface Identity {
  getAttributeValues(attributeName: AmUserAttribute): JavaArray<string>;
  setAttribute(attributeName: AmUserAttribute, attributeValues: string[]): void;
  addAttribute(attributeName: AmUserAttribute, attributeValue: string): void;
}

// Localized-message helper (shape not enumerated in the editor metadata).
declare const locales: any;

// ---- action (ActionWrapper, all methods chain) ---------------------------
interface Action {
  /** Exit the node via the named outcome. */
  goTo(outcome: StringLike): Action;
  /** Identify the user for the rest of the journey. */
  withIdentifiedUser(username: StringLike): Action;
  /** Identify an agent for the rest of the journey. */
  withIdentifiedAgent(agentName: StringLike): Action;
  /** Suspend the journey; `additionalLogic` runs on resume. */
  suspend(callbackTextFormat: StringLike): Action;
  suspend(callbackTextFormat: StringLike, additionalLogic: object): Action;
  suspend(
    callbackTextFormat: StringLike,
    additionalLogic: object,
    maximumSuspendDuration: number
  ): Action;
  withHeader(header: StringLike): Action;
  withStage(stage: StringLike): Action;
  withDescription(description: StringLike): Action;
  withErrorMessage(errorMessage: StringLike): Action;
  withLockoutMessage(lockoutMessage: StringLike): Action;
  putSessionProperty(key: StringLike, value: StringLike): Action;
  removeSessionProperty(key: StringLike): Action;
  withMaxSessionTime(maxSessionTime: number): Action;
  withMaxIdleTime(maxIdleTime: number): Action;
}
declare const action: Action;

// ---- callbacksBuilder ----------------------------------------------------
interface CallbacksBuilder {
  /** Prompt for a username (`NameCallback`). */
  nameCallback(prompt: StringLike): void;
  nameCallback(prompt: StringLike, defaultName: StringLike): void;
  /** Prompt for a password; `echoOn` shows typed chars (`PasswordCallback`). */
  passwordCallback(prompt: StringLike, echoOn: boolean): void;
  /** Username with policy validation. */
  validatedUsernameCallback(
    prompt: StringLike,
    policies: object,
    validateOnly: boolean
  ): void;
  validatedUsernameCallback(
    prompt: StringLike,
    policies: object,
    validateOnly: boolean,
    failedPolicies: string[]
  ): void;
  /** Password with policy validation. */
  validatedPasswordCallback(
    prompt: StringLike,
    echoOn: boolean,
    policies: object,
    validateOnly: boolean
  ): void;
  validatedPasswordCallback(
    prompt: StringLike,
    echoOn: boolean,
    policies: object,
    validateOnly: boolean,
    failedPolicies: string[]
  ): void;
  /** Free-text input, optionally pre-filled (`TextInputCallback`). */
  textInputCallback(prompt: StringLike): void;
  textInputCallback(prompt: StringLike, defaultText: StringLike): void;
  /**
   * Display a message (`TextOutputCallback`).
   * @param messageType 0 Information · 1 Warning · 2 Error · 4 Script
   */
  textOutputCallback(messageType: number, message: StringLike): void;
  /** Message shown while the journey is suspended (note: takes a messageType). */
  suspendedTextOutputCallback(messageType: number, message: StringLike): void;
  /** Inject client-side JavaScript (`ScriptTextOutputCallback`). */
  scriptTextOutputCallback(message: string): void;
  /** Round-trip a hidden value with the form (`HiddenValueCallback`). */
  hiddenValueCallback(id: StringLike, value: StringLike): void;
  /**
   * Confirmation buttons (`ConfirmationCallback`). `options` are button labels;
   * `optionType` selects a standard button set; `defaultOption` is the index.
   * @param messageType 0 Information · 1 Warning · 2 Error
   */
  confirmationCallback(
    messageType: number,
    options: string[],
    defaultOption: number
  ): void;
  confirmationCallback(
    messageType: number,
    optionType: number,
    defaultOption: number
  ): void;
  confirmationCallback(
    prompt: StringLike,
    messageType: number,
    options: string[],
    defaultOption: number
  ): void;
  confirmationCallback(
    prompt: StringLike,
    messageType: number,
    optionType: number,
    defaultOption: number
  ): void;
  /** Single- or multi-select choice list (`ChoiceCallback`). */
  choiceCallback(
    prompt: StringLike,
    choices: string[],
    defaultChoice: number,
    multipleSelectionsAllowed: boolean
  ): void;
  /** Spinner/poll; `waitTime` is milliseconds as a string. */
  pollingWaitCallback(waitTime: StringLike, message: StringLike): void;
  /** Browser redirect, optionally POSTing `redirectData`. */
  redirectCallback(
    redirectUrl: StringLike,
    redirectData: object,
    method: StringLike
  ): void;
  redirectCallback(
    redirectUrl: StringLike,
    redirectData: object,
    method: StringLike,
    setTrackingCookie: boolean
  ): void;
  redirectCallback(
    redirectUrl: StringLike,
    redirectData: object,
    method: StringLike,
    statusParameter: StringLike,
    redirectBackUrlCookie: StringLike
  ): void;
  redirectCallback(
    redirectUrl: StringLike,
    redirectData: object,
    method: StringLike,
    statusParameter: StringLike,
    redirectBackUrlCookie: StringLike,
    setTrackingCookie: boolean
  ): void;
  /** Arbitrary metadata returned to the client (`MetadataCallback`). */
  metadataCallback(outputValue: object): void;
  /** Language/locale selection. */
  languageCallback(language: StringLike, country: StringLike): void;
  /** Terms & conditions acceptance. */
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
  /** Knowledge-based-auth question setup. */
  kbaCreateCallback(
    prompt: StringLike,
    predefinedQuestions: string[],
    allowUserDefinedQuestions: boolean
  ): void;
  /** Social/IDP selection. */
  selectIdPCallback(providers: object): void;
  /** Social IDP redirect/native flow. */
  idPCallback(
    provider: StringLike,
    clientId: StringLike,
    redirectUri: StringLike,
    scope: string[],
    nonce: StringLike,
    request: StringLike,
    requestUri: StringLike,
    acrValues: string[],
    requestNativeAppForUserInfo: boolean
  ): void;
  idPCallback(
    provider: StringLike,
    clientId: StringLike,
    redirectUri: StringLike,
    scope: string[],
    nonce: StringLike,
    request: StringLike,
    requestUri: StringLike,
    acrValues: string[],
    requestNativeAppForUserInfo: boolean,
    token: StringLike,
    tokenType: StringLike
  ): void;
  /** Consent mapping prompt. */
  consentMappingCallback(
    name: StringLike,
    displayName: StringLike,
    icon: StringLike,
    accessLevel: StringLike,
    titles: object[],
    message: StringLike,
    isRequired: boolean
  ): void;
  consentMappingCallback(
    config: object,
    message: StringLike,
    isRequired: boolean
  ): void;
  /** HTTP (negotiate/SPNEGO) callback. */
  httpCallback(
    authRHeader: StringLike,
    negoName: StringLike,
    negoValue: StringLike,
    errorCode: number
  ): void;
  httpCallback(
    authorizationHeader: StringLike,
    negotiationHeader: StringLike,
    errorCode: StringLike
  ): void;
  /** X.509 client-certificate callback. */
  x509CertificateCallback(prompt: StringLike): void;
  x509CertificateCallback(prompt: StringLike, certificate: object): void;
  x509CertificateCallback(
    prompt: StringLike,
    certificate: object,
    requestSignature: boolean
  ): void;
  /** String attribute input; `policies` is an IDM policy object. */
  stringAttributeInputCallback(
    name: StringLike,
    prompt: StringLike,
    value: StringLike,
    required: boolean
  ): void;
  stringAttributeInputCallback(
    name: StringLike,
    prompt: StringLike,
    value: StringLike,
    required: boolean,
    failedPolicies: string[]
  ): void;
  stringAttributeInputCallback(
    name: StringLike,
    prompt: StringLike,
    value: StringLike,
    required: boolean,
    policies: object,
    validateOnly: boolean
  ): void;
  stringAttributeInputCallback(
    name: StringLike,
    prompt: StringLike,
    value: StringLike,
    required: boolean,
    policies: object,
    validateOnly: boolean,
    failedPolicies: string[]
  ): void;
  /** Numeric attribute input. */
  numberAttributeInputCallback(
    name: StringLike,
    prompt: StringLike,
    value: number,
    required: boolean
  ): void;
  numberAttributeInputCallback(
    name: StringLike,
    prompt: StringLike,
    value: number,
    required: boolean,
    failedPolicies: string[]
  ): void;
  numberAttributeInputCallback(
    name: StringLike,
    prompt: StringLike,
    value: number,
    required: boolean,
    policies: object,
    validateOnly: boolean
  ): void;
  numberAttributeInputCallback(
    name: StringLike,
    prompt: StringLike,
    value: number,
    required: boolean,
    policies: object,
    validateOnly: boolean,
    failedPolicies: string[]
  ): void;
  /** Boolean attribute input. */
  booleanAttributeInputCallback(
    name: StringLike,
    prompt: StringLike,
    value: boolean,
    required: boolean
  ): void;
  booleanAttributeInputCallback(
    name: StringLike,
    prompt: StringLike,
    value: boolean,
    required: boolean,
    failedPolicies: string[]
  ): void;
  booleanAttributeInputCallback(
    name: StringLike,
    prompt: StringLike,
    value: boolean,
    required: boolean,
    policies: object,
    validateOnly: boolean
  ): void;
  booleanAttributeInputCallback(
    name: StringLike,
    prompt: StringLike,
    value: boolean,
    required: boolean,
    policies: object,
    validateOnly: boolean,
    failedPolicies: string[]
  ): void;
}
declare const callbacksBuilder: CallbacksBuilder;

declare const requestCookies: RequestMap;

// ---- other next-gen-only bindings ---------------------------------------

/** Persist values across journey instances. */
interface CacheManager {
  exists(cacheName: StringLike): boolean;
  named(cacheName: StringLike): object;
}
declare const cacheManager: CacheManager;

/** OAuth2/OIDC authorization context (when the journey is OAuth-associated). */
interface OAuthApplication {
  getApplicationId(): string;
  getClientProperties(): object;
  getRequestProperties(): object;
}
declare const oauthApplication: OAuthApplication;

/** SAML2 authentication-request context (when journey is SAML-associated). */
interface SamlApplication {
  getAssertion(): object;
  getApplicationId(): string;
  getAuthnRequest(): object;
  getIdpAttributes(): object;
  getSpAttributes(): object;
  getFlowInitiator(): string;
}
declare const samlApplication: SamlApplication;

/** Information about the current journey/tree. */
interface Journey {
  name(): string;
  innerJourney(): boolean;
  mustRun(): boolean;
  identityResource(): string;
}
declare const journey: Journey;
