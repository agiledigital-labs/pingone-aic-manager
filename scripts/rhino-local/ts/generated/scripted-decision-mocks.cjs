// GENERATED from docs/api/bindings/scripted-decision-next.json — do not edit.
// Re-run: npm --prefix scripts/rhino-local/ts run generate
//
// Evaluated by AM's Rhino 1.7.14 as part of the script-under-test's scope.
// AM-safe JavaScript only: no let, no top-level const, no for...of, no object
// shorthand, no destructuring, no default parameters, and none of Map / Set /
// Symbol / Promise / WeakMap / WeakSet. See docs/api/12-script-bindings-matrix.md.

function __rhinoLocalArgKind(value) {
  if (Array.isArray(value)) {
    return "array";
  }
  var t = typeof value;
  if (t === "string" || t === "number" || t === "boolean") {
    return t;
  }
  return "object";
}

function __rhinoLocalNotMocked(binding, method, args, signatures) {
  var arity = args.length;
  var matched = [];
  var i;
  var j;
  var sig;
  var typesOk;
  for (i = 0; i < signatures.length; i += 1) {
    sig = signatures[i];
    if (sig.arity !== arity) {
      continue;
    }
    typesOk = true;
    for (j = 0; j < arity; j += 1) {
      if (__rhinoLocalArgKind(args[j]) !== sig.types[j]) {
        typesOk = false;
        break;
      }
    }
    if (typesOk) {
      matched.push(sig.label);
    }
  }
  if (matched.length === 0) {
    for (i = 0; i < signatures.length; i += 1) {
      if (signatures[i].arity === arity) {
        matched.push(signatures[i].label);
      }
    }
  }
  if (matched.length === 0) {
    matched.push("no matching signature");
  }
  throw new Error(
    "rhino-local: not mocked: " +
      binding +
      "." +
      method +
      " arity=" +
      arity +
      " overload=[" +
      matched.join(" | ") +
      "]"
  );
}

var samlApplication = {
  getAssertion: function () {
    __rhinoLocalNotMocked("samlApplication", "getAssertion", arguments, [{ arity: 0, types: [], label: "getAssertion()" }]);
  },
  getApplicationId: function () {
    __rhinoLocalNotMocked("samlApplication", "getApplicationId", arguments, [{ arity: 0, types: [], label: "getApplicationId()" }]);
  },
  getAuthnRequest: function () {
    __rhinoLocalNotMocked("samlApplication", "getAuthnRequest", arguments, [{ arity: 0, types: [], label: "getAuthnRequest()" }]);
  },
  getIdpAttributes: function () {
    __rhinoLocalNotMocked("samlApplication", "getIdpAttributes", arguments, [{ arity: 0, types: [], label: "getIdpAttributes()" }]);
  },
  getSpAttributes: function () {
    __rhinoLocalNotMocked("samlApplication", "getSpAttributes", arguments, [{ arity: 0, types: [], label: "getSpAttributes()" }]);
  },
  getFlowInitiator: function () {
    __rhinoLocalNotMocked("samlApplication", "getFlowInitiator", arguments, [{ arity: 0, types: [], label: "getFlowInitiator()" }]);
  },
};

var logger = {
  getName: function () {
    __rhinoLocalNotMocked("logger", "getName", arguments, [{ arity: 0, types: [], label: "getName()" }]);
  },
  info: function () {
    __rhinoLocalNotMocked("logger", "info", arguments, [
      { arity: 2, types: ["string","object"], label: "info(format: string, arg: object)" },
      { arity: 3, types: ["string","object","object"], label: "info(format: string, arg1: object, arg2: object)" },
      { arity: 1, types: ["string"], label: "info(msg: string)" },
      { arity: 2, types: ["string","array"], label: "info(format: string, arguments: array)" },
      { arity: 2, types: ["string","object"], label: "info(msg: string, t: object)" },
    ]);
  },
  trace: function () {
    __rhinoLocalNotMocked("logger", "trace", arguments, [
      { arity: 3, types: ["string","object","object"], label: "trace(format: string, arg1: object, arg2: object)" },
      { arity: 1, types: ["string"], label: "trace(msg: string)" },
      { arity: 2, types: ["string","object"], label: "trace(format: string, arg: object)" },
      { arity: 2, types: ["string","array"], label: "trace(format: string, arguments: array)" },
      { arity: 2, types: ["string","object"], label: "trace(msg: string, t: object)" },
    ]);
  },
  error: function () {
    __rhinoLocalNotMocked("logger", "error", arguments, [
      { arity: 2, types: ["string","object"], label: "error(msg: string, t: object)" },
      { arity: 2, types: ["string","array"], label: "error(format: string, arguments: array)" },
      { arity: 3, types: ["string","object","object"], label: "error(format: string, arg1: object, arg2: object)" },
      { arity: 2, types: ["string","object"], label: "error(format: string, arg: object)" },
      { arity: 1, types: ["string"], label: "error(msg: string)" },
    ]);
  },
  warn: function () {
    __rhinoLocalNotMocked("logger", "warn", arguments, [
      { arity: 1, types: ["string"], label: "warn(msg: string)" },
      { arity: 2, types: ["string","object"], label: "warn(format: string, arg: object)" },
      { arity: 3, types: ["string","object","object"], label: "warn(format: string, arg1: object, arg2: object)" },
      { arity: 2, types: ["string","object"], label: "warn(msg: string, t: object)" },
      { arity: 2, types: ["string","array"], label: "warn(format: string, arguments: array)" },
    ]);
  },
  debug: function () {
    __rhinoLocalNotMocked("logger", "debug", arguments, [
      { arity: 3, types: ["string","object","object"], label: "debug(format: string, arg1: object, arg2: object)" },
      { arity: 2, types: ["string","object"], label: "debug(format: string, arg: object)" },
      { arity: 2, types: ["string","object"], label: "debug(msg: string, t: object)" },
      { arity: 2, types: ["string","array"], label: "debug(format: string, arguments: array)" },
      { arity: 1, types: ["string"], label: "debug(msg: string)" },
    ]);
  },
  isTraceEnabled: function () {
    __rhinoLocalNotMocked("logger", "isTraceEnabled", arguments, [{ arity: 0, types: [], label: "isTraceEnabled()" }]);
  },
  isDebugEnabled: function () {
    __rhinoLocalNotMocked("logger", "isDebugEnabled", arguments, [{ arity: 0, types: [], label: "isDebugEnabled()" }]);
  },
  isErrorEnabled: function () {
    __rhinoLocalNotMocked("logger", "isErrorEnabled", arguments, [{ arity: 0, types: [], label: "isErrorEnabled()" }]);
  },
  isInfoEnabled: function () {
    __rhinoLocalNotMocked("logger", "isInfoEnabled", arguments, [{ arity: 0, types: [], label: "isInfoEnabled()" }]);
  },
  isWarnEnabled: function () {
    __rhinoLocalNotMocked("logger", "isWarnEnabled", arguments, [{ arity: 0, types: [], label: "isWarnEnabled()" }]);
  },
};

var callbacks = {
  isEmpty: function () {
    __rhinoLocalNotMocked("callbacks", "isEmpty", arguments, [{ arity: 0, types: [], label: "isEmpty()" }]);
  },
  getNameCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getNameCallbacks", arguments, [{ arity: 0, types: [], label: "getNameCallbacks()" }]);
  },
  getPasswordCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getPasswordCallbacks", arguments, [{ arity: 0, types: [], label: "getPasswordCallbacks()" }]);
  },
  getHiddenValueCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getHiddenValueCallbacks", arguments, [{ arity: 0, types: [], label: "getHiddenValueCallbacks()" }]);
  },
  getDeviceProfileCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getDeviceProfileCallbacks", arguments, [{ arity: 0, types: [], label: "getDeviceProfileCallbacks()" }]);
  },
  getKbaCreateCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getKbaCreateCallbacks", arguments, [{ arity: 0, types: [], label: "getKbaCreateCallbacks()" }]);
  },
  getSelectIdPCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getSelectIdPCallbacks", arguments, [{ arity: 0, types: [], label: "getSelectIdPCallbacks()" }]);
  },
  getTermsAndConditionsCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getTermsAndConditionsCallbacks", arguments, [{ arity: 0, types: [], label: "getTermsAndConditionsCallbacks()" }]);
  },
  getTextInputCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getTextInputCallbacks", arguments, [{ arity: 0, types: [], label: "getTextInputCallbacks()" }]);
  },
  getStringAttributeInputCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getStringAttributeInputCallbacks", arguments, [{ arity: 0, types: [], label: "getStringAttributeInputCallbacks()" }]);
  },
  getNumberAttributeInputCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getNumberAttributeInputCallbacks", arguments, [{ arity: 0, types: [], label: "getNumberAttributeInputCallbacks()" }]);
  },
  getBooleanAttributeInputCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getBooleanAttributeInputCallbacks", arguments, [{ arity: 0, types: [], label: "getBooleanAttributeInputCallbacks()" }]);
  },
  getConfirmationCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getConfirmationCallbacks", arguments, [{ arity: 0, types: [], label: "getConfirmationCallbacks()" }]);
  },
  getLanguageCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getLanguageCallbacks", arguments, [{ arity: 0, types: [], label: "getLanguageCallbacks()" }]);
  },
  getIdpCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getIdpCallbacks", arguments, [{ arity: 0, types: [], label: "getIdpCallbacks()" }]);
  },
  getValidatedPasswordCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getValidatedPasswordCallbacks", arguments, [{ arity: 0, types: [], label: "getValidatedPasswordCallbacks()" }]);
  },
  getValidatedUsernameCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getValidatedUsernameCallbacks", arguments, [{ arity: 0, types: [], label: "getValidatedUsernameCallbacks()" }]);
  },
  getHttpCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getHttpCallbacks", arguments, [{ arity: 0, types: [], label: "getHttpCallbacks()" }]);
  },
  getX509CertificateCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getX509CertificateCallbacks", arguments, [{ arity: 0, types: [], label: "getX509CertificateCallbacks()" }]);
  },
  getConsentMappingCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getConsentMappingCallbacks", arguments, [{ arity: 0, types: [], label: "getConsentMappingCallbacks()" }]);
  },
  getChoiceCallbacks: function () {
    __rhinoLocalNotMocked("callbacks", "getChoiceCallbacks", arguments, [{ arity: 0, types: [], label: "getChoiceCallbacks()" }]);
  },
};

var idRepository = {
  getIdentity: function () {
    __rhinoLocalNotMocked("idRepository", "getIdentity", arguments, [{ arity: 1, types: ["string"], label: "getIdentity(userName: string)" }]);
  },
};

var jwtAssertion = {
  generateJwt: function () {
    __rhinoLocalNotMocked("jwtAssertion", "generateJwt", arguments, [{ arity: 1, types: ["object"], label: "generateJwt(jwtData: object)" }]);
  },
};

var utils = {
  crypto: {
    randomUUID: function () {
      __rhinoLocalNotMocked("utils.crypto", "randomUUID", arguments, [{ arity: 0, types: [], label: "randomUUID()" }]);
    },
    getRandomValues: function () {
      __rhinoLocalNotMocked("utils.crypto", "getRandomValues", arguments, [{ arity: 1, types: ["array"], label: "getRandomValues(array: array)" }]);
    },
    subtle: {
      sign: function () {
        __rhinoLocalNotMocked("utils.crypto.subtle", "sign", arguments, [
          { arity: 3, types: ["object","array","array"], label: "sign(algorithmOptions: object, key: array, data: array)" },
          { arity: 3, types: ["string","array","array"], label: "sign(algorithm: string, key: array, data: array)" },
        ]);
      },
      digest: function () {
        __rhinoLocalNotMocked("utils.crypto.subtle", "digest", arguments, [{ arity: 2, types: ["string","array"], label: "digest(algorithm: string, data: array)" }]);
      },
      verify: function () {
        __rhinoLocalNotMocked("utils.crypto.subtle", "verify", arguments, [
          { arity: 4, types: ["string","array","array","array"], label: "verify(algorithm: string, key: array, data: array, signature: array)" },
          { arity: 4, types: ["object","array","array","array"], label: "verify(algorithmOptions: object, key: array, data: array, signature: array)" },
        ]);
      },
      decrypt: function () {
        __rhinoLocalNotMocked("utils.crypto.subtle", "decrypt", arguments, [
          { arity: 3, types: ["string","array","array"], label: "decrypt(algorithm: string, key: array, data: array)" },
          { arity: 3, types: ["object","array","array"], label: "decrypt(algorithmOptions: object, key: array, data: array)" },
        ]);
      },
      encrypt: function () {
        __rhinoLocalNotMocked("utils.crypto.subtle", "encrypt", arguments, [
          { arity: 3, types: ["string","array","array"], label: "encrypt(algorithm: string, key: array, data: array)" },
          { arity: 3, types: ["object","array","array"], label: "encrypt(algorithmOptions: object, key: array, data: array)" },
        ]);
      },
      generateKey: function () {
        __rhinoLocalNotMocked("utils.crypto.subtle", "generateKey", arguments, [
          { arity: 1, types: ["object"], label: "generateKey(algorithm: object)" },
          { arity: 1, types: ["string"], label: "generateKey(algorithm: string)" },
        ]);
      },
      deriveKey: function () {
        __rhinoLocalNotMocked("utils.crypto.subtle", "deriveKey", arguments, [
          { arity: 3, types: ["string","array","number"], label: "deriveKey(algorithmName: string, baseKey: array, derivedKeyLength: number)" },
          { arity: 3, types: ["object","array","number"], label: "deriveKey(algorithm: object, baseKey: array, derivedKeyLength: number)" },
        ]);
      },
    },
  },
  base64: {
    decode: function () {
      __rhinoLocalNotMocked("utils.base64", "decode", arguments, [{ arity: 1, types: ["string"], label: "decode(toDecode: string)" }]);
    },
    encode: function () {
      __rhinoLocalNotMocked("utils.base64", "encode", arguments, [
        { arity: 1, types: ["string"], label: "encode(toEncode: string)" },
        { arity: 1, types: ["array"], label: "encode(toEncode: array)" },
      ]);
    },
    decodeToBytes: function () {
      __rhinoLocalNotMocked("utils.base64", "decodeToBytes", arguments, [{ arity: 1, types: ["string"], label: "decodeToBytes(toDecode: string)" }]);
    },
    btoa: function () {
      __rhinoLocalNotMocked("utils.base64", "btoa", arguments, [{ arity: 1, types: ["string"], label: "btoa(toEncode: string)" }]);
    },
    atob: function () {
      __rhinoLocalNotMocked("utils.base64", "atob", arguments, [{ arity: 1, types: ["string"], label: "atob(toDecode: string)" }]);
    },
  },
  base64url: {
    decode: function () {
      __rhinoLocalNotMocked("utils.base64url", "decode", arguments, [{ arity: 1, types: ["string"], label: "decode(toDecode: string)" }]);
    },
    encode: function () {
      __rhinoLocalNotMocked("utils.base64url", "encode", arguments, [
        { arity: 1, types: ["string"], label: "encode(toEncode: string)" },
        { arity: 1, types: ["array"], label: "encode(toEncode: array)" },
      ]);
    },
    decodeToBytes: function () {
      __rhinoLocalNotMocked("utils.base64url", "decodeToBytes", arguments, [{ arity: 1, types: ["string"], label: "decodeToBytes(toDecode: string)" }]);
    },
    btoa: function () {
      __rhinoLocalNotMocked("utils.base64url", "btoa", arguments, [{ arity: 1, types: ["string"], label: "btoa(toEncode: string)" }]);
    },
    atob: function () {
      __rhinoLocalNotMocked("utils.base64url", "atob", arguments, [{ arity: 1, types: ["string"], label: "atob(toDecode: string)" }]);
    },
  },
  types: {
    bytesToString: function () {
      __rhinoLocalNotMocked("utils.types", "bytesToString", arguments, [{ arity: 1, types: ["array"], label: "bytesToString(bytes: array)" }]);
    },
    stringToBytes: function () {
      __rhinoLocalNotMocked("utils.types", "stringToBytes", arguments, [{ arity: 1, types: ["string"], label: "stringToBytes(string: string)" }]);
    },
  },
};

var action = {
  withIdentifiedUser: function () {
    __rhinoLocalNotMocked("action", "withIdentifiedUser", arguments, [{ arity: 1, types: ["string"], label: "withIdentifiedUser(username: string)" }]);
  },
  withIdentifiedAgent: function () {
    __rhinoLocalNotMocked("action", "withIdentifiedAgent", arguments, [{ arity: 1, types: ["string"], label: "withIdentifiedAgent(agentName: string)" }]);
  },
  goTo: function () {
    __rhinoLocalNotMocked("action", "goTo", arguments, [{ arity: 1, types: ["string"], label: "goTo(outcome: string)" }]);
  },
  suspend: function () {
    __rhinoLocalNotMocked("action", "suspend", arguments, [
      { arity: 3, types: ["string","object","number"], label: "suspend(callbackTextFormat: string, additionalLogic: object, maximumSuspendDuration: number)" },
      { arity: 1, types: ["string"], label: "suspend(callbackTextFormat: string)" },
      { arity: 2, types: ["string","object"], label: "suspend(callbackTextFormat: string, additionalLogic: object)" },
    ]);
  },
  withHeader: function () {
    __rhinoLocalNotMocked("action", "withHeader", arguments, [{ arity: 1, types: ["string"], label: "withHeader(header: string)" }]);
  },
  withStage: function () {
    __rhinoLocalNotMocked("action", "withStage", arguments, [{ arity: 1, types: ["string"], label: "withStage(stage: string)" }]);
  },
  putSessionProperty: function () {
    __rhinoLocalNotMocked("action", "putSessionProperty", arguments, [{ arity: 2, types: ["string","string"], label: "putSessionProperty(key: string, value: string)" }]);
  },
  withDescription: function () {
    __rhinoLocalNotMocked("action", "withDescription", arguments, [{ arity: 1, types: ["string"], label: "withDescription(description: string)" }]);
  },
  withErrorMessage: function () {
    __rhinoLocalNotMocked("action", "withErrorMessage", arguments, [{ arity: 1, types: ["string"], label: "withErrorMessage(errorMessage: string)" }]);
  },
  withLockoutMessage: function () {
    __rhinoLocalNotMocked("action", "withLockoutMessage", arguments, [{ arity: 1, types: ["string"], label: "withLockoutMessage(lockoutMessage: string)" }]);
  },
  removeSessionProperty: function () {
    __rhinoLocalNotMocked("action", "removeSessionProperty", arguments, [{ arity: 1, types: ["string"], label: "removeSessionProperty(key: string)" }]);
  },
  withMaxSessionTime: function () {
    __rhinoLocalNotMocked("action", "withMaxSessionTime", arguments, [{ arity: 1, types: ["number"], label: "withMaxSessionTime(maxSessionTime: number)" }]);
  },
  withMaxIdleTime: function () {
    __rhinoLocalNotMocked("action", "withMaxIdleTime", arguments, [{ arity: 1, types: ["number"], label: "withMaxIdleTime(maxIdleTime: number)" }]);
  },
};

var callbacksBuilder = {
  suspendedTextOutputCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "suspendedTextOutputCallback", arguments, [{ arity: 2, types: ["number","string"], label: "suspendedTextOutputCallback(messageType: number, message: string)" }]);
  },
  textInputCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "textInputCallback", arguments, [
      { arity: 2, types: ["string","string"], label: "textInputCallback(prompt: string, defaultText: string)" },
      { arity: 1, types: ["string"], label: "textInputCallback(prompt: string)" },
    ]);
  },
  scriptTextOutputCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "scriptTextOutputCallback", arguments, [{ arity: 1, types: ["string"], label: "scriptTextOutputCallback(message: string)" }]);
  },
  languageCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "languageCallback", arguments, [{ arity: 2, types: ["string","string"], label: "languageCallback(language: string, country: string)" }]);
  },
  idPCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "idPCallback", arguments, [
      { arity: 9, types: ["string","string","string","array","string","string","string","array","boolean"], label: "idPCallback(provider: string, clientId: string, redirectUri: string, scope: array, nonce: string, request: string, requestUri: string, acrValues: array, requestNativeAppForUserInfo: boolean)" },
      { arity: 11, types: ["string","string","string","array","string","string","string","array","boolean","string","string"], label: "idPCallback(provider: string, clientId: string, redirectUri: string, scope: array, nonce: string, request: string, requestUri: string, acrValues: array, requestNativeAppForUserInfo: boolean, token: string, tokenType: string)" },
    ]);
  },
  httpCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "httpCallback", arguments, [
      { arity: 4, types: ["string","string","string","number"], label: "httpCallback(authRHeader: string, negoName: string, negoValue: string, errorCode: number)" },
      { arity: 3, types: ["string","string","string"], label: "httpCallback(authorizationHeader: string, negotiationHeader: string, errorCode: string)" },
    ]);
  },
  x509CertificateCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "x509CertificateCallback", arguments, [
      { arity: 3, types: ["string","object","boolean"], label: "x509CertificateCallback(prompt: string, certificate: object, requestSignature: boolean)" },
      { arity: 2, types: ["string","object"], label: "x509CertificateCallback(prompt: string, certificate: object)" },
      { arity: 1, types: ["string"], label: "x509CertificateCallback(prompt: string)" },
    ]);
  },
  consentMappingCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "consentMappingCallback", arguments, [
      { arity: 7, types: ["string","string","string","string","array","string","boolean"], label: "consentMappingCallback(name: string, displayName: string, icon: string, accessLevel: string, titles: array, message: string, isRequired: boolean)" },
      { arity: 3, types: ["object","string","boolean"], label: "consentMappingCallback(config: object, message: string, isRequired: boolean)" },
    ]);
  },
  deviceProfileCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "deviceProfileCallback", arguments, [{ arity: 3, types: ["boolean","boolean","string"], label: "deviceProfileCallback(metadata: boolean, location: boolean, message: string)" }]);
  },
  kbaCreateCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "kbaCreateCallback", arguments, [{ arity: 3, types: ["string","array","boolean"], label: "kbaCreateCallback(prompt: string, predefinedQuestions: array, allowUserDefinedQuestions: boolean)" }]);
  },
  selectIdPCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "selectIdPCallback", arguments, [{ arity: 1, types: ["object"], label: "selectIdPCallback(providers: object)" }]);
  },
  termsAndConditionsCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "termsAndConditionsCallback", arguments, [{ arity: 3, types: ["string","string","string"], label: "termsAndConditionsCallback(version: string, terms: string, createDate: string)" }]);
  },
  metadataCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "metadataCallback", arguments, [{ arity: 1, types: ["object"], label: "metadataCallback(outputValue: object)" }]);
  },
  stringAttributeInputCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "stringAttributeInputCallback", arguments, [
      { arity: 7, types: ["string","string","string","boolean","object","boolean","array"], label: "stringAttributeInputCallback(name: string, prompt: string, value: string, required: boolean, policies: object, validateOnly: boolean, failedPolicies: array)" },
      { arity: 4, types: ["string","string","string","boolean"], label: "stringAttributeInputCallback(name: string, prompt: string, value: string, required: boolean)" },
      { arity: 5, types: ["string","string","string","boolean","array"], label: "stringAttributeInputCallback(name: string, prompt: string, value: string, required: boolean, failedPolicies: array)" },
      { arity: 6, types: ["string","string","string","boolean","object","boolean"], label: "stringAttributeInputCallback(name: string, prompt: string, value: string, required: boolean, policies: object, validateOnly: boolean)" },
    ]);
  },
  numberAttributeInputCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "numberAttributeInputCallback", arguments, [
      { arity: 7, types: ["string","string","number","boolean","object","boolean","array"], label: "numberAttributeInputCallback(name: string, prompt: string, value: number, required: boolean, policies: object, validateOnly: boolean, failedPolicies: array)" },
      { arity: 4, types: ["string","string","number","boolean"], label: "numberAttributeInputCallback(name: string, prompt: string, value: number, required: boolean)" },
      { arity: 5, types: ["string","string","number","boolean","array"], label: "numberAttributeInputCallback(name: string, prompt: string, value: number, required: boolean, failedPolicies: array)" },
      { arity: 6, types: ["string","string","number","boolean","object","boolean"], label: "numberAttributeInputCallback(name: string, prompt: string, value: number, required: boolean, policies: object, validateOnly: boolean)" },
    ]);
  },
  booleanAttributeInputCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "booleanAttributeInputCallback", arguments, [
      { arity: 7, types: ["string","string","boolean","boolean","object","boolean","array"], label: "booleanAttributeInputCallback(name: string, prompt: string, value: boolean, required: boolean, policies: object, validateOnly: boolean, failedPolicies: array)" },
      { arity: 4, types: ["string","string","boolean","boolean"], label: "booleanAttributeInputCallback(name: string, prompt: string, value: boolean, required: boolean)" },
      { arity: 5, types: ["string","string","boolean","boolean","array"], label: "booleanAttributeInputCallback(name: string, prompt: string, value: boolean, required: boolean, failedPolicies: array)" },
      { arity: 6, types: ["string","string","boolean","boolean","object","boolean"], label: "booleanAttributeInputCallback(name: string, prompt: string, value: boolean, required: boolean, policies: object, validateOnly: boolean)" },
    ]);
  },
  pollingWaitCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "pollingWaitCallback", arguments, [{ arity: 2, types: ["string","string"], label: "pollingWaitCallback(waitTime: string, message: string)" }]);
  },
  confirmationCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "confirmationCallback", arguments, [
      { arity: 3, types: ["number","array","number"], label: "confirmationCallback(messageType: number, options: array, defaultOption: number)" },
      { arity: 4, types: ["string","number","array","number"], label: "confirmationCallback(prompt: string, messageType: number, options: array, defaultOption: number)" },
      { arity: 4, types: ["string","number","number","number"], label: "confirmationCallback(prompt: string, messageType: number, optionType: number, defaultOption: number)" },
      { arity: 3, types: ["number","number","number"], label: "confirmationCallback(messageType: number, optionType: number, defaultOption: number)" },
    ]);
  },
  textOutputCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "textOutputCallback", arguments, [{ arity: 2, types: ["number","string"], label: "textOutputCallback(messageType: number, message: string)" }]);
  },
  choiceCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "choiceCallback", arguments, [{ arity: 4, types: ["string","array","number","boolean"], label: "choiceCallback(prompt: string, choices: array, defaultChoice: number, multipleSelectionsAllowed: boolean)" }]);
  },
  redirectCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "redirectCallback", arguments, [
      { arity: 3, types: ["string","object","string"], label: "redirectCallback(redirectUrl: string, redirectData: object, method: string)" },
      { arity: 6, types: ["string","object","string","string","string","boolean"], label: "redirectCallback(redirectUrl: string, redirectData: object, method: string, statusParameter: string, redirectBackUrlCookie: string, setTrackingCookie: boolean)" },
      { arity: 4, types: ["string","object","string","boolean"], label: "redirectCallback(redirectUrl: string, redirectData: object, method: string, setTrackingCookie: boolean)" },
      { arity: 5, types: ["string","object","string","string","string"], label: "redirectCallback(redirectUrl: string, redirectData: object, method: string, statusParameter: string, redirectBackUrlCookie: string)" },
    ]);
  },
  hiddenValueCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "hiddenValueCallback", arguments, [{ arity: 2, types: ["string","string"], label: "hiddenValueCallback(id: string, value: string)" }]);
  },
  nameCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "nameCallback", arguments, [
      { arity: 2, types: ["string","string"], label: "nameCallback(prompt: string, defaultName: string)" },
      { arity: 1, types: ["string"], label: "nameCallback(prompt: string)" },
    ]);
  },
  passwordCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "passwordCallback", arguments, [{ arity: 2, types: ["string","boolean"], label: "passwordCallback(prompt: string, echoOn: boolean)" }]);
  },
  validatedUsernameCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "validatedUsernameCallback", arguments, [
      { arity: 3, types: ["string","object","boolean"], label: "validatedUsernameCallback(prompt: string, policies: object, validateOnly: boolean)" },
      { arity: 4, types: ["string","object","boolean","array"], label: "validatedUsernameCallback(prompt: string, policies: object, validateOnly: boolean, failedPolicies: array)" },
    ]);
  },
  validatedPasswordCallback: function () {
    __rhinoLocalNotMocked("callbacksBuilder", "validatedPasswordCallback", arguments, [
      { arity: 4, types: ["string","boolean","object","boolean"], label: "validatedPasswordCallback(prompt: string, echoOn: boolean, policies: object, validateOnly: boolean)" },
      { arity: 5, types: ["string","boolean","object","boolean","array"], label: "validatedPasswordCallback(prompt: string, echoOn: boolean, policies: object, validateOnly: boolean, failedPolicies: array)" },
    ]);
  },
};

var openidm = {
  update: function () {
    __rhinoLocalNotMocked("openidm", "update", arguments, [
      { arity: 5, types: ["string","string","object","object","array"], label: "update(id: string, rev: string, value: object, params: object, fields: array)" },
      { arity: 4, types: ["string","string","object","object"], label: "update(id: string, rev: string, value: object, params: object)" },
      { arity: 3, types: ["string","string","object"], label: "update(id: string, rev: string, value: object)" },
    ]);
  },
  action: function () {
    __rhinoLocalNotMocked("openidm", "action", arguments, [
      { arity: 4, types: ["string","string","object","object"], label: "action(resource: string, actionName: string, content: object, params: object)" },
      { arity: 5, types: ["string","string","object","object","array"], label: "action(resource: string, actionName: string, content: object, params: object, fields: array)" },
      { arity: 3, types: ["string","string","object"], label: "action(resource: string, actionName: string, content: object)" },
      { arity: 2, types: ["string","string"], label: "action(resource: string, actionName: string)" },
    ]);
  },
  create: function () {
    __rhinoLocalNotMocked("openidm", "create", arguments, [
      { arity: 3, types: ["string","string","object"], label: "create(resourceName: string, newResourceId: string, content: object)" },
      { arity: 4, types: ["string","string","object","object"], label: "create(resourceName: string, newResourceId: string, content: object, params: object)" },
      { arity: 5, types: ["string","string","object","object","array"], label: "create(resourceName: string, newResourceId: string, content: object, params: object, fields: array)" },
    ]);
  },
  delete: function () {
    __rhinoLocalNotMocked("openidm", "delete", arguments, [
      { arity: 3, types: ["string","string","object"], label: "delete(resourceName: string, rev: string, params: object)" },
      { arity: 4, types: ["string","string","object","array"], label: "delete(resourceName: string, rev: string, params: object, fields: array)" },
      { arity: 2, types: ["string","string"], label: "delete(resourceName: string, rev: string)" },
    ]);
  },
  read: function () {
    __rhinoLocalNotMocked("openidm", "read", arguments, [
      { arity: 1, types: ["string"], label: "read(resourceName: string)" },
      { arity: 2, types: ["string","object"], label: "read(resourceName: string, params: object)" },
      { arity: 3, types: ["string","object","array"], label: "read(resourceName: string, params: object, fields: array)" },
    ]);
  },
  query: function () {
    __rhinoLocalNotMocked("openidm", "query", arguments, [
      { arity: 3, types: ["string","object","array"], label: "query(resourceName: string, params: object, fields: array)" },
      { arity: 2, types: ["string","object"], label: "query(resourceName: string, params: object)" },
    ]);
  },
  patch: function () {
    __rhinoLocalNotMocked("openidm", "patch", arguments, [
      { arity: 5, types: ["string","string","array","object","array"], label: "patch(resourceName: string, rev: string, patch: array, params: object, fields: array)" },
      { arity: 3, types: ["string","string","array"], label: "patch(resourceName: string, rev: string, patch: array)" },
      { arity: 4, types: ["string","string","array","object"], label: "patch(resourceName: string, rev: string, patch: array, params: object)" },
    ]);
  },
};

// Opaque container: the contexts metadata lists no elements. A case
// seeds this object directly; a later slice may replace the assignment.
var requestCookies = {};

var cookieName = "__rhino-local-unseeded__";

var policy = {
  evaluate: function () {
    __rhinoLocalNotMocked("policy", "evaluate", arguments, [{ arity: 4, types: ["object","string","array","object"], label: "evaluate(subject: object, application: string, resourceNames: array, environment: object)" }]);
  },
};

var httpClient = {
  send: function () {
    __rhinoLocalNotMocked("httpClient", "send", arguments, [
      { arity: 2, types: ["string","object"], label: "send(uri: string, requestOptions: object)" },
      { arity: 1, types: ["string"], label: "send(uri: string)" },
    ]);
  },
};

var journey = {
  name: function () {
    __rhinoLocalNotMocked("journey", "name", arguments, [{ arity: 0, types: [], label: "name()" }]);
  },
  innerJourney: function () {
    __rhinoLocalNotMocked("journey", "innerJourney", arguments, [{ arity: 0, types: [], label: "innerJourney()" }]);
  },
  mustRun: function () {
    __rhinoLocalNotMocked("journey", "mustRun", arguments, [{ arity: 0, types: [], label: "mustRun()" }]);
  },
  identityResource: function () {
    __rhinoLocalNotMocked("journey", "identityResource", arguments, [{ arity: 0, types: [], label: "identityResource()" }]);
  },
};

// Opaque container: the contexts metadata lists no elements. A case
// seeds this object directly; a later slice may replace the assignment.
var requestParameters = {};

var cacheManager = {
  exists: function () {
    __rhinoLocalNotMocked("cacheManager", "exists", arguments, [{ arity: 1, types: ["string"], label: "exists(cacheName: string)" }]);
  },
  named: function () {
    __rhinoLocalNotMocked("cacheManager", "named", arguments, [{ arity: 1, types: ["string"], label: "named(cacheName: string)" }]);
  },
};

var secrets = {
  getGenericSecret: function () {
    __rhinoLocalNotMocked("secrets", "getGenericSecret", arguments, [{ arity: 1, types: ["string"], label: "getGenericSecret(secretId: string)" }]);
  },
  getDecryptionKey: function () {
    __rhinoLocalNotMocked("secrets", "getDecryptionKey", arguments, [{ arity: 1, types: ["string"], label: "getDecryptionKey(secretId: string)" }]);
  },
  getEncryptionKey: function () {
    __rhinoLocalNotMocked("secrets", "getEncryptionKey", arguments, [{ arity: 1, types: ["string"], label: "getEncryptionKey(secretId: string)" }]);
  },
  getSigningKey: function () {
    __rhinoLocalNotMocked("secrets", "getSigningKey", arguments, [{ arity: 1, types: ["string"], label: "getSigningKey(secretId: string)" }]);
  },
  getVerificationKey: function () {
    __rhinoLocalNotMocked("secrets", "getVerificationKey", arguments, [{ arity: 1, types: ["string"], label: "getVerificationKey(secretId: string)" }]);
  },
};

var oauthApplication = {
  getApplicationId: function () {
    __rhinoLocalNotMocked("oauthApplication", "getApplicationId", arguments, [{ arity: 0, types: [], label: "getApplicationId()" }]);
  },
  getClientProperties: function () {
    __rhinoLocalNotMocked("oauthApplication", "getClientProperties", arguments, [{ arity: 0, types: [], label: "getClientProperties()" }]);
  },
  getRequestProperties: function () {
    __rhinoLocalNotMocked("oauthApplication", "getRequestProperties", arguments, [{ arity: 0, types: [], label: "getRequestProperties()" }]);
  },
};

// Opaque container: the contexts metadata lists no elements. A case
// seeds this object directly; a later slice may replace the assignment.
var locales = {};

// Opaque container: the contexts metadata lists no elements. A case
// seeds this object directly; a later slice may replace the assignment.
var requestHeaders = {};

var nodeState = {
  remove: function () {
    __rhinoLocalNotMocked("nodeState", "remove", arguments, [{ arity: 1, types: ["string"], label: "remove(key: string)" }]);
  },
  get: function () {
    __rhinoLocalNotMocked("nodeState", "get", arguments, [{ arity: 1, types: ["string"], label: "get(key: string)" }]);
  },
  keys: function () {
    __rhinoLocalNotMocked("nodeState", "keys", arguments, [{ arity: 0, types: [], label: "keys()" }]);
  },
  isDefined: function () {
    __rhinoLocalNotMocked("nodeState", "isDefined", arguments, [{ arity: 1, types: ["string"], label: "isDefined(key: string)" }]);
  },
  getObject: function () {
    __rhinoLocalNotMocked("nodeState", "getObject", arguments, [{ arity: 1, types: ["string"], label: "getObject(key: string)" }]);
  },
  putTransient: function () {
    __rhinoLocalNotMocked("nodeState", "putTransient", arguments, [{ arity: 2, types: ["string","object"], label: "putTransient(key: string, value: object)" }]);
  },
  putShared: function () {
    __rhinoLocalNotMocked("nodeState", "putShared", arguments, [{ arity: 2, types: ["string","object"], label: "putShared(key: string, value: object)" }]);
  },
  mergeShared: function () {
    __rhinoLocalNotMocked("nodeState", "mergeShared", arguments, [{ arity: 1, types: ["object"], label: "mergeShared(object: object)" }]);
  },
  mergeTransient: function () {
    __rhinoLocalNotMocked("nodeState", "mergeTransient", arguments, [{ arity: 1, types: ["object"], label: "mergeTransient(object: object)" }]);
  },
};

var resumedFromSuspend = false;

var scriptName = "__rhino-local-unseeded__";

var realm = "__rhino-local-unseeded__";

var jwtValidator = {
  validateJwtClaims: function () {
    __rhinoLocalNotMocked("jwtValidator", "validateJwtClaims", arguments, [{ arity: 1, types: ["object"], label: "validateJwtClaims(jwtData: object)" }]);
  },
};
