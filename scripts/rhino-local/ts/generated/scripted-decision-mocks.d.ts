// GENERATED from docs/api/bindings/scripted-decision-next.json — do not edit.
// Re-run: npm --prefix scripts/rhino-local/ts run generate
//
// Case-authoring types for the scripted-decision mock surface. javaScriptType
// values are mapped as-is (object → object, array → unknown[]); they do not
// include the Rhino Java interop layer in src/scripts/templates/am/types/.
// Every generated method throws at runtime until a later slice implements it.

/**
 * Binding whose contexts metadata lists an empty `elements` array.
 * A test case seeds this object directly; the generator does not invent
 * methods the JSON does not name.
 */
export type OpaqueBinding = {
  [key: string]: unknown;
};

export interface SamlApplication {
  getAssertion(): object;
  getApplicationId(): string;
  getAuthnRequest(): object;
  getIdpAttributes(): object;
  getSpAttributes(): object;
  getFlowInitiator(): string;
}

export interface Logger {
  getName(): string;
  info(format: string, arg: object): void;
  info(format: string, arg1: object, arg2: object): void;
  info(msg: string): void;
  info(format: string, args: unknown[]): void;
  info(msg: string, t: object): void;
  trace(format: string, arg1: object, arg2: object): void;
  trace(msg: string): void;
  trace(format: string, arg: object): void;
  trace(format: string, args: unknown[]): void;
  trace(msg: string, t: object): void;
  error(msg: string, t: object): void;
  error(format: string, args: unknown[]): void;
  error(format: string, arg1: object, arg2: object): void;
  error(format: string, arg: object): void;
  error(msg: string): void;
  warn(msg: string): void;
  warn(format: string, arg: object): void;
  warn(format: string, arg1: object, arg2: object): void;
  warn(msg: string, t: object): void;
  warn(format: string, args: unknown[]): void;
  debug(format: string, arg1: object, arg2: object): void;
  debug(format: string, arg: object): void;
  debug(msg: string, t: object): void;
  debug(format: string, args: unknown[]): void;
  debug(msg: string): void;
  isTraceEnabled(): boolean;
  isDebugEnabled(): boolean;
  isErrorEnabled(): boolean;
  isInfoEnabled(): boolean;
  isWarnEnabled(): boolean;
}

export interface Callbacks {
  isEmpty(): boolean;
  getNameCallbacks(): unknown[];
  getPasswordCallbacks(): unknown[];
  getHiddenValueCallbacks(): object;
  getDeviceProfileCallbacks(): unknown[];
  getKbaCreateCallbacks(): unknown[];
  getSelectIdPCallbacks(): unknown[];
  getTermsAndConditionsCallbacks(): unknown[];
  getTextInputCallbacks(): unknown[];
  getStringAttributeInputCallbacks(): unknown[];
  getNumberAttributeInputCallbacks(): unknown[];
  getBooleanAttributeInputCallbacks(): unknown[];
  getConfirmationCallbacks(): unknown[];
  getLanguageCallbacks(): unknown[];
  getIdpCallbacks(): unknown[];
  getValidatedPasswordCallbacks(): unknown[];
  getValidatedUsernameCallbacks(): unknown[];
  getHttpCallbacks(): unknown[];
  getX509CertificateCallbacks(): unknown[];
  getConsentMappingCallbacks(): unknown[];
  getChoiceCallbacks(): unknown[];
}

export interface IdRepository {
  getIdentity(userName: string): object;
}

export interface JwtAssertion {
  generateJwt(jwtData: object): string;
}

export interface UtilsCryptoSubtle {
  sign(algorithmOptions: object, key: unknown[], data: unknown[]): unknown[];
  sign(algorithm: string, key: unknown[], data: unknown[]): unknown[];
  digest(algorithm: string, data: unknown[]): unknown[];
  verify(
    algorithm: string,
    key: unknown[],
    data: unknown[],
    signature: unknown[]
  ): boolean;
  verify(
    algorithmOptions: object,
    key: unknown[],
    data: unknown[],
    signature: unknown[]
  ): boolean;
  decrypt(algorithm: string, key: unknown[], data: unknown[]): unknown[];
  decrypt(algorithmOptions: object, key: unknown[], data: unknown[]): unknown[];
  encrypt(algorithm: string, key: unknown[], data: unknown[]): unknown[];
  encrypt(algorithmOptions: object, key: unknown[], data: unknown[]): unknown[];
  generateKey(algorithm: object): object;
  generateKey(algorithm: string): object;
  deriveKey(
    algorithmName: string,
    baseKey: unknown[],
    derivedKeyLength: number
  ): unknown[];
  deriveKey(
    algorithm: object,
    baseKey: unknown[],
    derivedKeyLength: number
  ): unknown[];
}

export interface UtilsCrypto {
  randomUUID(): string;
  getRandomValues(array: unknown[]): unknown[];
  subtle: UtilsCryptoSubtle;
}

export interface UtilsBase64 {
  decode(toDecode: string): string;
  encode(toEncode: string): string;
  encode(toEncode: unknown[]): string;
  decodeToBytes(toDecode: string): unknown[];
  btoa(toEncode: string): string;
  atob(toDecode: string): string;
}

export interface UtilsBase64url {
  decode(toDecode: string): string;
  encode(toEncode: string): string;
  encode(toEncode: unknown[]): string;
  decodeToBytes(toDecode: string): unknown[];
  btoa(toEncode: string): string;
  atob(toDecode: string): string;
}

export interface UtilsTypes {
  bytesToString(bytes: unknown[]): string;
  stringToBytes(string: string): unknown[];
}

export interface Utils {
  crypto: UtilsCrypto;
  base64: UtilsBase64;
  base64url: UtilsBase64url;
  types: UtilsTypes;
}

export interface Action {
  withIdentifiedUser(username: string): object;
  withIdentifiedAgent(agentName: string): object;
  goTo(outcome: string): object;
  suspend(
    callbackTextFormat: string,
    additionalLogic: object,
    maximumSuspendDuration: number
  ): object;
  suspend(callbackTextFormat: string): object;
  suspend(callbackTextFormat: string, additionalLogic: object): object;
  withHeader(header: string): object;
  withStage(stage: string): object;
  putSessionProperty(key: string, value: string): object;
  withDescription(description: string): object;
  withErrorMessage(errorMessage: string): object;
  withLockoutMessage(lockoutMessage: string): object;
  removeSessionProperty(key: string): object;
  withMaxSessionTime(maxSessionTime: number): object;
  withMaxIdleTime(maxIdleTime: number): object;
}

export interface CallbacksBuilder {
  suspendedTextOutputCallback(messageType: number, message: string): void;
  textInputCallback(prompt: string, defaultText: string): void;
  textInputCallback(prompt: string): void;
  scriptTextOutputCallback(message: string): void;
  languageCallback(language: string, country: string): void;
  idPCallback(
    provider: string,
    clientId: string,
    redirectUri: string,
    scope: unknown[],
    nonce: string,
    request: string,
    requestUri: string,
    acrValues: unknown[],
    requestNativeAppForUserInfo: boolean
  ): void;
  idPCallback(
    provider: string,
    clientId: string,
    redirectUri: string,
    scope: unknown[],
    nonce: string,
    request: string,
    requestUri: string,
    acrValues: unknown[],
    requestNativeAppForUserInfo: boolean,
    token: string,
    tokenType: string
  ): void;
  httpCallback(
    authRHeader: string,
    negoName: string,
    negoValue: string,
    errorCode: number
  ): void;
  httpCallback(
    authorizationHeader: string,
    negotiationHeader: string,
    errorCode: string
  ): void;
  x509CertificateCallback(
    prompt: string,
    certificate: object,
    requestSignature: boolean
  ): void;
  x509CertificateCallback(prompt: string, certificate: object): void;
  x509CertificateCallback(prompt: string): void;
  consentMappingCallback(
    name: string,
    displayName: string,
    icon: string,
    accessLevel: string,
    titles: unknown[],
    message: string,
    isRequired: boolean
  ): void;
  consentMappingCallback(
    config: object,
    message: string,
    isRequired: boolean
  ): void;
  deviceProfileCallback(
    metadata: boolean,
    location: boolean,
    message: string
  ): void;
  kbaCreateCallback(
    prompt: string,
    predefinedQuestions: unknown[],
    allowUserDefinedQuestions: boolean
  ): void;
  selectIdPCallback(providers: object): void;
  termsAndConditionsCallback(
    version: string,
    terms: string,
    createDate: string
  ): void;
  metadataCallback(outputValue: object): void;
  stringAttributeInputCallback(
    name: string,
    prompt: string,
    value: string,
    required: boolean,
    policies: object,
    validateOnly: boolean,
    failedPolicies: unknown[]
  ): void;
  stringAttributeInputCallback(
    name: string,
    prompt: string,
    value: string,
    required: boolean
  ): void;
  stringAttributeInputCallback(
    name: string,
    prompt: string,
    value: string,
    required: boolean,
    failedPolicies: unknown[]
  ): void;
  stringAttributeInputCallback(
    name: string,
    prompt: string,
    value: string,
    required: boolean,
    policies: object,
    validateOnly: boolean
  ): void;
  numberAttributeInputCallback(
    name: string,
    prompt: string,
    value: number,
    required: boolean,
    policies: object,
    validateOnly: boolean,
    failedPolicies: unknown[]
  ): void;
  numberAttributeInputCallback(
    name: string,
    prompt: string,
    value: number,
    required: boolean
  ): void;
  numberAttributeInputCallback(
    name: string,
    prompt: string,
    value: number,
    required: boolean,
    failedPolicies: unknown[]
  ): void;
  numberAttributeInputCallback(
    name: string,
    prompt: string,
    value: number,
    required: boolean,
    policies: object,
    validateOnly: boolean
  ): void;
  booleanAttributeInputCallback(
    name: string,
    prompt: string,
    value: boolean,
    required: boolean,
    policies: object,
    validateOnly: boolean,
    failedPolicies: unknown[]
  ): void;
  booleanAttributeInputCallback(
    name: string,
    prompt: string,
    value: boolean,
    required: boolean
  ): void;
  booleanAttributeInputCallback(
    name: string,
    prompt: string,
    value: boolean,
    required: boolean,
    failedPolicies: unknown[]
  ): void;
  booleanAttributeInputCallback(
    name: string,
    prompt: string,
    value: boolean,
    required: boolean,
    policies: object,
    validateOnly: boolean
  ): void;
  pollingWaitCallback(waitTime: string, message: string): void;
  confirmationCallback(
    messageType: number,
    options: unknown[],
    defaultOption: number
  ): void;
  confirmationCallback(
    prompt: string,
    messageType: number,
    options: unknown[],
    defaultOption: number
  ): void;
  confirmationCallback(
    prompt: string,
    messageType: number,
    optionType: number,
    defaultOption: number
  ): void;
  confirmationCallback(
    messageType: number,
    optionType: number,
    defaultOption: number
  ): void;
  textOutputCallback(messageType: number, message: string): void;
  choiceCallback(
    prompt: string,
    choices: unknown[],
    defaultChoice: number,
    multipleSelectionsAllowed: boolean
  ): void;
  redirectCallback(
    redirectUrl: string,
    redirectData: object,
    method: string
  ): void;
  redirectCallback(
    redirectUrl: string,
    redirectData: object,
    method: string,
    statusParameter: string,
    redirectBackUrlCookie: string,
    setTrackingCookie: boolean
  ): void;
  redirectCallback(
    redirectUrl: string,
    redirectData: object,
    method: string,
    setTrackingCookie: boolean
  ): void;
  redirectCallback(
    redirectUrl: string,
    redirectData: object,
    method: string,
    statusParameter: string,
    redirectBackUrlCookie: string
  ): void;
  hiddenValueCallback(id: string, value: string): void;
  nameCallback(prompt: string, defaultName: string): void;
  nameCallback(prompt: string): void;
  passwordCallback(prompt: string, echoOn: boolean): void;
  validatedUsernameCallback(
    prompt: string,
    policies: object,
    validateOnly: boolean
  ): void;
  validatedUsernameCallback(
    prompt: string,
    policies: object,
    validateOnly: boolean,
    failedPolicies: unknown[]
  ): void;
  validatedPasswordCallback(
    prompt: string,
    echoOn: boolean,
    policies: object,
    validateOnly: boolean
  ): void;
  validatedPasswordCallback(
    prompt: string,
    echoOn: boolean,
    policies: object,
    validateOnly: boolean,
    failedPolicies: unknown[]
  ): void;
}

export interface Openidm {
  update(
    id: string,
    rev: string,
    value: object,
    params: object,
    fields: unknown[]
  ): object;
  update(id: string, rev: string, value: object, params: object): object;
  update(id: string, rev: string, value: object): object;
  action(
    resource: string,
    actionName: string,
    content: object,
    params: object
  ): object;
  action(
    resource: string,
    actionName: string,
    content: object,
    params: object,
    fields: unknown[]
  ): object;
  action(resource: string, actionName: string, content: object): object;
  action(resource: string, actionName: string): object;
  create(resourceName: string, newResourceId: string, content: object): object;
  create(
    resourceName: string,
    newResourceId: string,
    content: object,
    params: object
  ): object;
  create(
    resourceName: string,
    newResourceId: string,
    content: object,
    params: object,
    fields: unknown[]
  ): object;
  delete(resourceName: string, rev: string, params: object): object;
  delete(
    resourceName: string,
    rev: string,
    params: object,
    fields: unknown[]
  ): object;
  delete(resourceName: string, rev: string): object;
  read(resourceName: string): object;
  read(resourceName: string, params: object): object;
  read(resourceName: string, params: object, fields: unknown[]): object;
  query(resourceName: string, params: object, fields: unknown[]): object;
  query(resourceName: string, params: object): object;
  patch(
    resourceName: string,
    rev: string,
    patch: unknown[],
    params: object,
    fields: unknown[]
  ): object;
  patch(resourceName: string, rev: string, patch: unknown[]): object;
  patch(
    resourceName: string,
    rev: string,
    patch: unknown[],
    params: object
  ): object;
}

export interface Policy {
  evaluate(
    subject: object,
    application: string,
    resourceNames: unknown[],
    environment: object
  ): unknown[];
}

export interface HttpClient {
  send(uri: string, requestOptions: object): object;
  send(uri: string): object;
}

export interface Journey {
  name(): string;
  innerJourney(): boolean;
  mustRun(): boolean;
  identityResource(): string;
}

export interface CacheManager {
  exists(cacheName: string): boolean;
  named(cacheName: string): object;
}

export interface Secrets {
  getGenericSecret(secretId: string): object;
  getDecryptionKey(secretId: string): object;
  getEncryptionKey(secretId: string): object;
  getSigningKey(secretId: string): object;
  getVerificationKey(secretId: string): object;
}

export interface OauthApplication {
  getApplicationId(): string;
  getClientProperties(): object;
  getRequestProperties(): object;
}

export interface NodeState {
  remove(key: string): void;
  get(key: string): object;
  keys(): object;
  isDefined(key: string): boolean;
  getObject(key: string): object;
  putTransient(key: string, value: object): object;
  putShared(key: string, value: object): object;
  mergeShared(object: object): object;
  mergeTransient(object: object): object;
}

export interface JwtValidator {
  validateJwtClaims(jwtData: object): object;
}

export interface ScriptedDecisionMocks {
  samlApplication: SamlApplication;
  logger: Logger;
  callbacks: Callbacks;
  idRepository: IdRepository;
  jwtAssertion: JwtAssertion;
  utils: Utils;
  action: Action;
  callbacksBuilder: CallbacksBuilder;
  openidm: Openidm;
  requestCookies: OpaqueBinding;
  cookieName: string;
  policy: Policy;
  httpClient: HttpClient;
  journey: Journey;
  requestParameters: OpaqueBinding;
  cacheManager: CacheManager;
  secrets: Secrets;
  oauthApplication: OauthApplication;
  locales: OpaqueBinding;
  requestHeaders: OpaqueBinding;
  nodeState: NodeState;
  resumedFromSuspend: boolean;
  scriptName: string;
  realm: string;
  jwtValidator: JwtValidator;
}
