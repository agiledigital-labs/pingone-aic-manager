// GENERATED from docs/api/bindings/device-match-next.json by
// scripts/gen-binding-types.mjs — do not edit by hand. Context: DEVICE_MATCH_NODE.
// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.
// Applied metadata refinements:
//   - Fluent builder methods return their own interface, not metadata's bare `object`.

interface SamlApplication {
  getAssertion(): object;
  getApplicationId(): StringLike;
  getAuthnRequest(): object;
  getIdpAttributes(): object;
  getSpAttributes(): object;
  getFlowInitiator(): StringLike;
}
declare const samlApplication: SamlApplication;

interface DeviceProfilesDao {
  getDeviceProfiles(username: StringLike, realm: StringLike): any[];
  saveDeviceProfiles(
    username: StringLike,
    realm: StringLike,
    deviceProfiles: any[]
  ): void;
}
declare const deviceProfilesDao: DeviceProfilesDao;

interface Callbacks {
  isEmpty(): boolean;
  getNameCallbacks(): any[];
  getPasswordCallbacks(): any[];
  getHiddenValueCallbacks(): object;
  getDeviceProfileCallbacks(): any[];
  getKbaCreateCallbacks(): any[];
  getSelectIdPCallbacks(): any[];
  getTermsAndConditionsCallbacks(): any[];
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
  getChoiceCallbacks(): any[];
}
declare const callbacks: Callbacks;

interface IdRepository {
  getIdentity(userName: StringLike): object;
}
declare const idRepository: IdRepository;

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

interface CallbacksBuilder {
  suspendedTextOutputCallback(messageType: number, message: StringLike): void;
  textInputCallback(prompt: StringLike, defaultText: StringLike): void;
  textInputCallback(prompt: StringLike): void;
  scriptTextOutputCallback(message: StringLike): void;
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
    name: StringLike,
    displayName: StringLike,
    icon: StringLike,
    accessLevel: StringLike,
    titles: any[],
    message: StringLike,
    isRequired: boolean
  ): void;
  consentMappingCallback(
    config: object,
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
    required: boolean
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
    failedPolicies: any[]
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
    failedPolicies: any[]
  ): void;
  numberAttributeInputCallback(
    name: StringLike,
    prompt: StringLike,
    value: number,
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
    failedPolicies: any[]
  ): void;
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
  pollingWaitCallback(waitTime: StringLike, message: StringLike): void;
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
  textOutputCallback(messageType: number, message: StringLike): void;
  choiceCallback(
    prompt: StringLike,
    choices: any[],
    defaultChoice: number,
    multipleSelectionsAllowed: boolean
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
  redirectCallback(
    redirectUrl: StringLike,
    redirectData: object,
    method: StringLike,
    statusParameter: StringLike,
    redirectBackUrlCookie: StringLike
  ): void;
  hiddenValueCallback(id: StringLike, value: StringLike): void;
  nameCallback(prompt: StringLike, defaultName: StringLike): void;
  nameCallback(prompt: StringLike): void;
  passwordCallback(prompt: StringLike, echoOn: boolean): void;
  validatedUsernameCallback(
    prompt: StringLike,
    policies: object,
    validateOnly: boolean
  ): void;
  validatedUsernameCallback(
    prompt: StringLike,
    policies: object,
    validateOnly: boolean,
    failedPolicies: any[]
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

declare const requestCookies: RequestMap;
interface Journey {
  name(): StringLike;
  innerJourney(): boolean;
  mustRun(): boolean;
  identityResource(): StringLike;
}
declare const journey: Journey;

declare const requestParameters: RequestMap;
interface OauthApplication {
  getApplicationId(): StringLike;
  getClientProperties(): object;
  getRequestProperties(): object;
}
declare const oauthApplication: OauthApplication;

declare const locales: any;
declare const requestHeaders: RequestMap;
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

declare const resumedFromSuspend: boolean;
