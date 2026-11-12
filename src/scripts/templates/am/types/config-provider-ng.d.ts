// GENERATED from docs/api/bindings/config-provider-node-next.json by
// scripts/gen-binding-types.mjs — do not edit by hand. Context: CONFIG_PROVIDER_NODE_NEXT_GEN.
// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.
// Applied metadata refinements:
//   - Fluent builder methods return their own interface, not metadata's bare `object`.
//   - `nodeState.get` returns `any`, `put*` take `any`, and `keys` is not declared (it throws on next-gen).
//   - NodeState members other than `get`, `isDefined`, `putShared`, `putTransient` are the descriptor's, unmeasured here.

interface SamlApplication {
  getApplicationId(): StringLike;
  getAuthnRequest(): object;
  getIdpAttributes(): object;
  getSpAttributes(): object;
  getFlowInitiator(): StringLike;
  getAssertion(): object;
}
declare const samlApplication: SamlApplication;

interface Callbacks {
  isEmpty(): boolean;
  getChoiceCallbacks(): any[];
  getNameCallbacks(): any[];
  getPasswordCallbacks(): any[];
  getHiddenValueCallbacks(): object;
  getTextInputCallbacks(): any[];
  getStringAttributeInputCallbacks(): any[];
  getNumberAttributeInputCallbacks(): any[];
  getBooleanAttributeInputCallbacks(): any[];
  getConfirmationCallbacks(): any[];
  getLanguageCallbacks(): any[];
  getIdpCallbacks(): any[];
  getValidatedPasswordCallbacks(): any[];
  getValidatedUsernameCallbacks(): any[];
  getHttpCallbacks(): any[];
  getX509CertificateCallbacks(): any[];
  getConsentMappingCallbacks(): any[];
  getDeviceProfileCallbacks(): any[];
  getKbaCreateCallbacks(): any[];
  getSelectIdPCallbacks(): any[];
  getTermsAndConditionsCallbacks(): any[];
}
declare const callbacks: Callbacks;

interface IdRepository {
  createUser(userName: StringLike, password: StringLike): object;
  createUser(
    userName: StringLike,
    password: StringLike,
    attributes: object
  ): object;
  getIdentity(userName: StringLike): object;
}
declare const idRepository: IdRepository;

// `existingSession` is listed by the descriptor but is a ReferenceError on a request
// with no session cookie (measured 2026-11-10); with one it is unmeasured here.
// Guard with `typeof existingSession !== "undefined"` before reading it.
declare const existingSession: object | undefined;
interface Action {
  withIdentifiedUser(username: StringLike): Action;
  withIdentifiedAgent(agentName: StringLike): Action;
  goTo(outcome: StringLike): Action;
  suspend(callbackTextFormat: StringLike): Action;
  suspend(
    callbackTextFormat: StringLike,
    additionalLogic: object,
    maximumSuspendDuration: number
  ): Action;
  suspend(callbackTextFormat: StringLike, additionalLogic: object): Action;
  putSessionProperty(key: StringLike, value: StringLike): Action;
  withHeader(header: StringLike): Action;
  withDescription(description: StringLike): Action;
  withStage(stage: StringLike): Action;
  withErrorMessage(errorMessage: StringLike): Action;
  withLockoutMessage(lockoutMessage: StringLike): Action;
  removeSessionProperty(key: StringLike): Action;
  withMaxSessionTime(maxSessionTime: number): Action;
  withMaxIdleTime(maxIdleTime: number): Action;
}
declare const action: Action;

interface CallbacksBuilder {
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
    policies: object,
    validateOnly: boolean,
    failedPolicies: any[]
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
    failedPolicies: any[]
  ): void;
  languageCallback(language: StringLike, country: StringLike): void;
  idPCallback(
    provider: StringLike,
    clientId: StringLike,
    redirectUri: StringLike,
    scope: any[],
    nonce: StringLike,
    request: StringLike,
    requestUri: StringLike,
    acrValues: any[],
    requestNativeAppForUserInfo: boolean
  ): void;
  idPCallback(
    provider: StringLike,
    clientId: StringLike,
    redirectUri: StringLike,
    scope: any[],
    nonce: StringLike,
    request: StringLike,
    requestUri: StringLike,
    acrValues: any[],
    requestNativeAppForUserInfo: boolean,
    token: StringLike,
    tokenType: StringLike
  ): void;
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
  x509CertificateCallback(
    prompt: StringLike,
    certificate: object,
    requestSignature: boolean
  ): void;
  x509CertificateCallback(prompt: StringLike, certificate: object): void;
  x509CertificateCallback(prompt: StringLike): void;
  consentMappingCallback(
    config: object,
    message: StringLike,
    isRequired: boolean
  ): void;
  consentMappingCallback(
    name: StringLike,
    displayName: StringLike,
    icon: StringLike,
    accessLevel: StringLike,
    titles: any[],
    message: StringLike,
    isRequired: boolean
  ): void;
  deviceProfileCallback(
    metadata: boolean,
    location: boolean,
    message: StringLike
  ): void;
  kbaCreateCallback(
    prompt: StringLike,
    predefinedQuestions: any[],
    allowUserDefinedQuestions: boolean
  ): void;
  selectIdPCallback(providers: object): void;
  termsAndConditionsCallback(
    version: StringLike,
    terms: StringLike,
    createDate: StringLike
  ): void;
  radioChoiceCallback(
    prompt: StringLike,
    choices: any[],
    defaultChoice: number
  ): void;
  suspendedTextOutputCallback(messageType: number, message: StringLike): void;
  textInputCallback(prompt: StringLike, defaultText: StringLike): void;
  textInputCallback(prompt: StringLike): void;
  scriptTextOutputCallback(message: StringLike): void;
  metadataCallback(outputValue: object): void;
  stringAttributeInputCallback(
    name: StringLike,
    prompt: StringLike,
    value: StringLike,
    required: boolean,
    policies: object,
    validateOnly: boolean,
    failedPolicies: any[]
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
    failedPolicies: any[]
  ): void;
  stringAttributeInputCallback(
    name: StringLike,
    prompt: StringLike,
    value: StringLike,
    required: boolean
  ): void;
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
    policies: object,
    validateOnly: boolean
  ): void;
  numberAttributeInputCallback(
    name: StringLike,
    prompt: StringLike,
    value: number,
    required: boolean,
    failedPolicies: any[]
  ): void;
  numberAttributeInputCallback(
    name: StringLike,
    prompt: StringLike,
    value: number,
    required: boolean,
    policies: object,
    validateOnly: boolean,
    failedPolicies: any[]
  ): void;
  choiceCallback(
    prompt: StringLike,
    choices: any[],
    defaultChoice: number,
    multipleSelectionsAllowed: boolean
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
    method: StringLike
  ): void;
  redirectCallback(
    redirectUrl: StringLike,
    redirectData: object,
    method: StringLike,
    statusParameter: StringLike,
    redirectBackUrlCookie: StringLike,
    setTrackingCookie: boolean
  ): void;
  redirectCallback(
    redirectUrl: StringLike,
    redirectData: object,
    method: StringLike,
    setTrackingCookie: boolean
  ): void;
  hiddenValueCallback(id: StringLike, value: StringLike): void;
  nameCallback(prompt: StringLike, defaultName: StringLike): void;
  nameCallback(prompt: StringLike): void;
  passwordCallback(prompt: StringLike, echoOn: boolean): void;
  confirmationCallback(
    messageType: number,
    options: any[],
    defaultOption: number
  ): void;
  confirmationCallback(
    prompt: StringLike,
    messageType: number,
    options: any[],
    defaultOption: number
  ): void;
  confirmationCallback(
    prompt: StringLike,
    messageType: number,
    optionType: number,
    defaultOption: number
  ): void;
  confirmationCallback(
    messageType: number,
    optionType: number,
    defaultOption: number
  ): void;
  pollingWaitCallback(waitTime: StringLike, message: StringLike): void;
  qrCodeCallback(url: StringLike): void;
  textOutputCallback(messageType: number, message: StringLike): void;
  validatedUsernameCallback(
    prompt: StringLike,
    policies: object,
    validateOnly: boolean,
    failedPolicies: any[]
  ): void;
  validatedUsernameCallback(
    prompt: StringLike,
    policies: object,
    validateOnly: boolean
  ): void;
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
    failedPolicies: any[]
  ): void;
}
declare const callbacksBuilder: CallbacksBuilder;

declare const requestCookies: RequestCookieMap;
interface Journey {
  name(): StringLike;
  innerJourney(): boolean;
  mustRun(): boolean;
  identityResource(): StringLike;
}
declare const journey: Journey;

declare const requestParameters: RequestMap;
interface OauthApplication {
  getRequestProperties(): object;
  getApplicationId(): StringLike;
  getClientProperties(): object;
}
declare const oauthApplication: OauthApplication;

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

declare const locales: any;
declare const requestHeaders: RequestMap;
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

declare const resumedFromSuspend: boolean;

// AM reads the script's `config` global AFTER the script ends, and the wrapped node
// is configured from it. A plain object is accepted (`{}` gives the node's defaults);
// never assigned, `undefined`, `null`, a string, a number, a boolean, an array or a
// function is a 500 with an empty body. `config` is a ReferenceError until assigned
// (measured 2026-11-10, docs/api/12). Numbers become doubles, and `new
// java.util.HashMap()` is not available on next-gen.
interface ConfigProviderConfig {
  [key: string]: any;
}
declare let config: ConfigProviderConfig | undefined;
