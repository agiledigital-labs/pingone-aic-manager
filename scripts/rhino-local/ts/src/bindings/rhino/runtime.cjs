// Handwritten overlay on generated/scripted-decision-mocks.cjs.
// Evaluated inside AM's Rhino 1.7.14 (VERSION_DEFAULT): no let, no top-level
// const, no for...of, no object shorthand, no destructuring, no default
// parameters, and none of Map / Set / Symbol / Promise / WeakMap / WeakSet.
//
// Methods this file does not replace keep throwing
// `rhino-local: not mocked: <binding>.<method> …`.

var __rhinoLocal = {
  shared: {},
  transient: {},
  secure: {},
  initialShared: {},
  initialTransient: {},
  initialSecure: {},
  managed: {},
  esv: {},
  secrets: {},
  submittedCallbacks: null,
  httpStubs: [],
  generatedId: 0,
  outcome: null,
  callbacks: [],
  openidm: [],
  http: [],
  logs: [],
  libraries: {},
  requireCache: {},
};

var sharedState;
var transientState;
var systemEnv;
var require;

function __rhinoLocalClone(value) {
  return JSON.parse(JSON.stringify(value));
}

function __rhinoLocalHide(object, name, fn) {
  if (typeof Object.defineProperty === "function") {
    Object.defineProperty(object, name, {
      enumerable: false,
      configurable: true,
      writable: true,
      value: fn,
    });
    return;
  }
  object[name] = fn;
}

function __rhinoLocalExpectArity(path, args, expected) {
  if (args.length !== expected) {
    throw new Error(
      "rhino-local: " +
        path +
        " arity=" +
        args.length +
        " (expected " +
        expected +
        ")"
    );
  }
}

function __rhinoLocalHas(object, key) {
  return Object.prototype.hasOwnProperty.call(object, key);
}

function __rhinoLocalIsPlainObject(value) {
  return (
    value !== null &&
    typeof value === "object" &&
    !Array.isArray(value)
  );
}

function __rhinoLocalAssignPlain(target, source) {
  if (!__rhinoLocalIsPlainObject(source)) {
    return;
  }
  var keys = Object.keys(source);
  var i;
  for (i = 0; i < keys.length; i += 1) {
    target[keys[i]] = source[keys[i]];
  }
}

function __rhinoLocalMergeBucket(bucketName, object) {
  if (!__rhinoLocalIsPlainObject(object)) {
    throw new Error(
      "rhino-local: nodeState.merge" +
        (bucketName === "shared" ? "Shared" : "Transient") +
        ": expected an object"
    );
  }
  var bucket = __rhinoLocal[bucketName];
  var keys = Object.keys(object);
  var i;
  var key;
  var incoming;
  var existing;
  var merged;
  for (i = 0; i < keys.length; i += 1) {
    key = keys[i];
    incoming = object[key];
    existing = bucket[key];
    if (__rhinoLocalIsPlainObject(existing) && __rhinoLocalIsPlainObject(incoming)) {
      merged = {};
      __rhinoLocalAssignPlain(merged, existing);
      __rhinoLocalAssignPlain(merged, incoming);
      bucket[key] = merged;
    } else {
      bucket[key] = incoming;
    }
  }
}

function __rhinoLocalJavaList(items) {
  var list = [];
  var i;
  for (i = 0; i < items.length; i += 1) {
    list.push(items[i]);
  }
  __rhinoLocalHide(list, "size", function () {
    return this.length;
  });
  __rhinoLocalHide(list, "get", function (index) {
    return this[index];
  });
  __rhinoLocalHide(list, "isEmpty", function () {
    return this.length === 0;
  });
  __rhinoLocalHide(list, "contains", function (value) {
    return this.indexOf(value) !== -1;
  });
  return list;
}

function __rhinoLocalRequestMap(values, options) {
  var caseInsensitive = Boolean(options && options.caseInsensitive);
  var asList = !(options && options.asList === false);
  var map = {};
  var store = {};
  var keyCount = 0;
  var names = values ? Object.keys(values) : [];
  var i;
  var raw;
  var stored;
  var value;
  for (i = 0; i < names.length; i += 1) {
    raw = names[i];
    stored = caseInsensitive ? String(raw).toLowerCase() : String(raw);
    value = values[raw];
    if (asList) {
      if (typeof value === "string") {
        store[stored] = __rhinoLocalJavaList([value]);
      } else {
        store[stored] = __rhinoLocalJavaList(value);
      }
    } else {
      store[stored] = value;
    }
    map[stored] = store[stored];
    keyCount += 1;
  }
  __rhinoLocalHide(map, "get", function (name) {
    __rhinoLocalExpectArity("map.get", arguments, 1);
    var key = caseInsensitive ? String(name).toLowerCase() : String(name);
    if (!__rhinoLocalHas(store, key)) {
      return null;
    }
    return store[key];
  });
  __rhinoLocalHide(map, "containsKey", function (name) {
    __rhinoLocalExpectArity("map.containsKey", arguments, 1);
    var key = caseInsensitive ? String(name).toLowerCase() : String(name);
    return __rhinoLocalHas(store, key);
  });
  __rhinoLocalHide(map, "size", function () {
    return keyCount;
  });
  __rhinoLocalHide(map, "isEmpty", function () {
    return keyCount === 0;
  });
  __rhinoLocalHide(map, "keySet", function () {
    throw new Error(
      "rhino-local: keySet is blocked by AM's class shutter; iterate with for...in"
    );
  });
  return map;
}

function __rhinoLocalStateMap(bucketName) {
  var map = {};
  __rhinoLocalHide(map, "get", function (key) {
    __rhinoLocalExpectArity(bucketName + ".get", arguments, 1);
    var k = String(key);
    if (!__rhinoLocalHas(__rhinoLocal[bucketName], k)) {
      return null;
    }
    return __rhinoLocal[bucketName][k];
  });
  __rhinoLocalHide(map, "put", function (key, value) {
    __rhinoLocalExpectArity(bucketName + ".put", arguments, 2);
    if (value === undefined) {
      throw new Error("rhino-local: " + bucketName + ".put: value is undefined");
    }
    __rhinoLocal[bucketName][String(key)] = value;
  });
  return map;
}

function __rhinoLocalLookupState(key) {
  if (__rhinoLocalHas(__rhinoLocal.transient, key)) {
    return __rhinoLocal.transient[key];
  }
  if (__rhinoLocalHas(__rhinoLocal.secure, key)) {
    return __rhinoLocal.secure[key];
  }
  if (__rhinoLocalHas(__rhinoLocal.shared, key)) {
    return __rhinoLocal.shared[key];
  }
  return null;
}

function __rhinoLocalIsDefined(key) {
  return (
    __rhinoLocalHas(__rhinoLocal.transient, key) ||
    __rhinoLocalHas(__rhinoLocal.secure, key) ||
    __rhinoLocalHas(__rhinoLocal.shared, key)
  );
}

nodeState.get = function (key) {
  __rhinoLocalExpectArity("nodeState.get", arguments, 1);
  return __rhinoLocalLookupState(String(key));
};

nodeState.isDefined = function (key) {
  __rhinoLocalExpectArity("nodeState.isDefined", arguments, 1);
  return __rhinoLocalIsDefined(String(key));
};

nodeState.putShared = function (key, value) {
  __rhinoLocalExpectArity("nodeState.putShared", arguments, 2);
  if (value === undefined) {
    throw new Error("rhino-local: nodeState.putShared: value is undefined");
  }
  __rhinoLocal.shared[String(key)] = value;
  return nodeState;
};

nodeState.putTransient = function (key, value) {
  __rhinoLocalExpectArity("nodeState.putTransient", arguments, 2);
  if (value === undefined) {
    throw new Error("rhino-local: nodeState.putTransient: value is undefined");
  }
  __rhinoLocal.transient[String(key)] = value;
  return nodeState;
};

nodeState.remove = function (key) {
  __rhinoLocalExpectArity("nodeState.remove", arguments, 1);
  var k = String(key);
  delete __rhinoLocal.shared[k];
  delete __rhinoLocal.transient[k];
  delete __rhinoLocal.secure[k];
};

nodeState.keys = function () {
  __rhinoLocalExpectArity("nodeState.keys", arguments, 0);
  var seen = {};
  var names = [];
  function take(bucket) {
    var keys = Object.keys(bucket);
    var i;
    var key;
    for (i = 0; i < keys.length; i += 1) {
      key = keys[i];
      if (!__rhinoLocalHas(seen, key)) {
        seen[key] = true;
        names.push(key);
      }
    }
  }
  take(__rhinoLocal.transient);
  take(__rhinoLocal.secure);
  take(__rhinoLocal.shared);
  return __rhinoLocalJavaList(names);
};

nodeState.getObject = function (key) {
  __rhinoLocalExpectArity("nodeState.getObject", arguments, 1);
  var k = String(key);
  var sharedVal = __rhinoLocalHas(__rhinoLocal.shared, k)
    ? __rhinoLocal.shared[k]
    : undefined;
  var secureVal = __rhinoLocalHas(__rhinoLocal.secure, k)
    ? __rhinoLocal.secure[k]
    : undefined;
  var transientVal = __rhinoLocalHas(__rhinoLocal.transient, k)
    ? __rhinoLocal.transient[k]
    : undefined;
  if (
    __rhinoLocalIsPlainObject(sharedVal) ||
    __rhinoLocalIsPlainObject(secureVal) ||
    __rhinoLocalIsPlainObject(transientVal)
  ) {
    var merged = {};
    __rhinoLocalAssignPlain(merged, sharedVal);
    __rhinoLocalAssignPlain(merged, secureVal);
    __rhinoLocalAssignPlain(merged, transientVal);
    return merged;
  }
  return __rhinoLocalLookupState(k);
};

nodeState.mergeShared = function (object) {
  __rhinoLocalExpectArity("nodeState.mergeShared", arguments, 1);
  __rhinoLocalMergeBucket("shared", object);
  return nodeState;
};

nodeState.mergeTransient = function (object) {
  __rhinoLocalExpectArity("nodeState.mergeTransient", arguments, 1);
  __rhinoLocalMergeBucket("transient", object);
  return nodeState;
};

action.goTo = function (name) {
  __rhinoLocalExpectArity("action.goTo", arguments, 1);
  __rhinoLocal.outcome = String(name);
  outcome = __rhinoLocal.outcome;
  return action;
};

action.withErrorMessage = function (message) {
  __rhinoLocalExpectArity("action.withErrorMessage", arguments, 1);
  return action;
};

action.withHeader = function (header) {
  __rhinoLocalExpectArity("action.withHeader", arguments, 1);
  return action;
};

action.withIdentifiedUser = function (username) {
  __rhinoLocalExpectArity("action.withIdentifiedUser", arguments, 1);
  return action;
};

action.withIdentifiedAgent = function (agentName) {
  __rhinoLocalExpectArity("action.withIdentifiedAgent", arguments, 1);
  return action;
};

action.withStage = function (stage) {
  __rhinoLocalExpectArity("action.withStage", arguments, 1);
  return action;
};

action.withDescription = function (description) {
  __rhinoLocalExpectArity("action.withDescription", arguments, 1);
  return action;
};

action.withLockoutMessage = function (lockoutMessage) {
  __rhinoLocalExpectArity("action.withLockoutMessage", arguments, 1);
  return action;
};

action.putSessionProperty = function (key, value) {
  __rhinoLocalExpectArity("action.putSessionProperty", arguments, 2);
  return action;
};

action.removeSessionProperty = function (key) {
  __rhinoLocalExpectArity("action.removeSessionProperty", arguments, 1);
  return action;
};

action.withMaxSessionTime = function (maxSessionTime) {
  __rhinoLocalExpectArity("action.withMaxSessionTime", arguments, 1);
  return action;
};

action.withMaxIdleTime = function (maxIdleTime) {
  __rhinoLocalExpectArity("action.withMaxIdleTime", arguments, 1);
  return action;
};

action.suspend = function (callbackTextFormat, additionalLogic) {
  if (arguments.length < 1 || arguments.length > 3) {
    throw new Error(
      "rhino-local: action.suspend arity=" +
        arguments.length +
        " (expected 1..3)"
    );
  }
  var message = String(callbackTextFormat);
  var resumeUri = "https://rhino-local.invalid/resume";
  if (arguments.length >= 2) {
    if (typeof additionalLogic !== "function") {
      throw new Error(
        "rhino-local: action.suspend additionalLogic is not a function"
      );
    }
    message = message.split("{0}").join(resumeUri);
    additionalLogic(resumeUri);
  }
  __rhinoLocalCallback("SuspendedTextOutputCallback", { message: message });
  return action;
};

function __rhinoLocalIsThrowable(value) {
  return (
    value !== null &&
    typeof value === "object" &&
    typeof value.getMessage === "function" &&
    typeof value.getStackTrace === "function"
  );
}

function __rhinoLocalFormat(pattern, bound) {
  var out = "";
  var i = 0;
  var argIndex = 0;
  var slashes;
  var pairs;
  var odd;
  while (i < pattern.length) {
    if (
      pattern.charAt(i) === "{" &&
      i + 1 < pattern.length &&
      pattern.charAt(i + 1) === "}"
    ) {
      slashes = 0;
      while (out.length - slashes > 0 && out.charAt(out.length - 1 - slashes) === "\\") {
        slashes += 1;
      }
      pairs = Math.floor(slashes / 2);
      odd = slashes % 2 === 1;
      out = out.substring(0, out.length - slashes);
      var p;
      for (p = 0; p < pairs; p += 1) {
        out += "\\";
      }
      if (odd) {
        out += "{}";
      } else if (argIndex < bound.length) {
        out += String(bound[argIndex]);
        argIndex += 1;
      } else {
        out += "{}";
      }
      i += 2;
    } else {
      out += pattern.charAt(i);
      i += 1;
    }
  }
  return out;
}

function __rhinoLocalLog(level, args) {
  if (args.length < 1) {
    throw new Error(
      "rhino-local: logger." + level + " arity=" + args.length + " (expected >= 1)"
    );
  }
  var format = String(args[0]);
  var bound = [];
  var i;
  if (args.length === 2 && Array.isArray(args[1])) {
    bound = args[1];
  } else {
    for (i = 1; i < args.length; i += 1) {
      bound.push(args[i]);
    }
  }
  if (bound.length > 0 && __rhinoLocalIsThrowable(bound[bound.length - 1])) {
    bound = bound.slice(0, bound.length - 1);
  }
  __rhinoLocal.logs.push({
    level: level,
    message: __rhinoLocalFormat(format, bound),
  });
}

logger.getName = function () {
  return scriptName;
};

logger.trace = function () {
  __rhinoLocalLog("trace", arguments);
};

logger.debug = function () {
  __rhinoLocalLog("debug", arguments);
};

logger.info = function () {
  __rhinoLocalLog("info", arguments);
};

logger.warn = function () {
  __rhinoLocalLog("warn", arguments);
};

logger.error = function () {
  __rhinoLocalLog("error", arguments);
};

logger.isTraceEnabled = function () {
  return true;
};

logger.isDebugEnabled = function () {
  return true;
};

logger.isInfoEnabled = function () {
  return true;
};

logger.isWarnEnabled = function () {
  return true;
};

logger.isErrorEnabled = function () {
  return true;
};

function __rhinoLocalCallback(type, fields) {
  var cb = { type: type };
  var keys = Object.keys(fields);
  var i;
  var key;
  for (i = 0; i < keys.length; i += 1) {
    key = keys[i];
    if (fields[key] !== undefined) {
      cb[key] = fields[key];
    }
  }
  __rhinoLocal.callbacks.push(cb);
}

callbacksBuilder.nameCallback = function (prompt, defaultName) {
  if (arguments.length !== 1 && arguments.length !== 2) {
    throw new Error(
      "rhino-local: callbacksBuilder.nameCallback arity=" +
        arguments.length +
        " (expected 1 or 2)"
    );
  }
  var fields = { prompt: String(prompt) };
  if (arguments.length > 1) {
    fields.defaultName = String(defaultName);
  }
  __rhinoLocalCallback("NameCallback", fields);
};

callbacksBuilder.passwordCallback = function (prompt, echoOn) {
  __rhinoLocalExpectArity("callbacksBuilder.passwordCallback", arguments, 2);
  __rhinoLocalCallback("PasswordCallback", {
    prompt: String(prompt),
    echoOn: Boolean(echoOn),
  });
};

callbacksBuilder.textOutputCallback = function (messageType, message) {
  __rhinoLocalExpectArity("callbacksBuilder.textOutputCallback", arguments, 2);
  __rhinoLocalCallback("TextOutputCallback", {
    messageType: messageType,
    message: String(message),
  });
};

callbacksBuilder.hiddenValueCallback = function (id, value) {
  __rhinoLocalExpectArity("callbacksBuilder.hiddenValueCallback", arguments, 2);
  __rhinoLocalCallback("HiddenValueCallback", {
    id: String(id),
    value: String(value),
  });
};

callbacksBuilder.confirmationCallback = function () {
  var a = arguments;
  var fields = {};
  if (a.length === 3 && typeof a[0] === "number" && Array.isArray(a[1])) {
    fields.messageType = a[0];
    fields.options = a[1];
    fields.defaultOption = a[2];
  } else if (a.length === 3 && typeof a[0] === "number") {
    fields.messageType = a[0];
    fields.optionType = a[1];
    fields.defaultOption = a[2];
  } else if (a.length === 4 && Array.isArray(a[2])) {
    fields.prompt = String(a[0]);
    fields.messageType = a[1];
    fields.options = a[2];
    fields.defaultOption = a[3];
  } else if (a.length === 4) {
    fields.prompt = String(a[0]);
    fields.messageType = a[1];
    fields.optionType = a[2];
    fields.defaultOption = a[3];
  } else {
    throw new Error(
      "rhino-local: callbacksBuilder.confirmationCallback arity=" + a.length
    );
  }
  __rhinoLocalCallback("ConfirmationCallback", fields);
};

callbacksBuilder.choiceCallback = function (
  prompt,
  choices,
  defaultChoice,
  multipleSelectionsAllowed
) {
  __rhinoLocalExpectArity("callbacksBuilder.choiceCallback", arguments, 4);
  __rhinoLocalCallback("ChoiceCallback", {
    prompt: String(prompt),
    choices: choices,
    defaultChoice: defaultChoice,
    multipleSelectionsAllowed: Boolean(multipleSelectionsAllowed),
  });
};

callbacksBuilder.suspendedTextOutputCallback = function (messageType, message) {
  __rhinoLocalExpectArity(
    "callbacksBuilder.suspendedTextOutputCallback",
    arguments,
    2
  );
  __rhinoLocalCallback("SuspendedTextOutputCallback", {
    messageType: messageType,
    message: String(message),
  });
};

callbacksBuilder.textInputCallback = function (prompt, defaultText) {
  if (arguments.length !== 1 && arguments.length !== 2) {
    throw new Error(
      "rhino-local: callbacksBuilder.textInputCallback arity=" +
        arguments.length +
        " (expected 1 or 2)"
    );
  }
  var textFields = { prompt: String(prompt) };
  if (arguments.length > 1) {
    textFields.defaultText = String(defaultText);
  }
  __rhinoLocalCallback("TextInputCallback", textFields);
};

callbacksBuilder.scriptTextOutputCallback = function (message) {
  __rhinoLocalExpectArity(
    "callbacksBuilder.scriptTextOutputCallback",
    arguments,
    1
  );
  __rhinoLocalCallback("ScriptTextOutputCallback", { message: String(message) });
};

callbacksBuilder.languageCallback = function (language, country) {
  __rhinoLocalExpectArity("callbacksBuilder.languageCallback", arguments, 2);
  __rhinoLocalCallback("LanguageCallback", {
    language: String(language),
    country: String(country),
  });
};

callbacksBuilder.idPCallback = function (
  provider,
  clientId,
  redirectUri,
  scope,
  nonce,
  request,
  requestUri,
  acrValues,
  requestNativeAppForUserInfo,
  token,
  tokenType
) {
  if (arguments.length !== 9 && arguments.length !== 11) {
    throw new Error(
      "rhino-local: callbacksBuilder.idPCallback arity=" +
        arguments.length +
        " (expected 9 or 11)"
    );
  }
  var idpFields = {
    provider: String(provider),
    clientId: String(clientId),
    redirectUri: String(redirectUri),
    scope: scope,
    nonce: String(nonce),
    request: String(request),
    requestUri: String(requestUri),
    acrValues: acrValues,
    requestNativeAppForUserInfo: Boolean(requestNativeAppForUserInfo),
  };
  if (arguments.length === 11) {
    idpFields.token = String(token);
    idpFields.tokenType = String(tokenType);
  }
  __rhinoLocalCallback("IdPCallback", idpFields);
};

callbacksBuilder.httpCallback = function () {
  var a = arguments;
  if (a.length === 4) {
    __rhinoLocalCallback("HttpCallback", {
      authRHeader: String(a[0]),
      negoName: String(a[1]),
      negoValue: String(a[2]),
      errorCode: a[3],
    });
    return;
  }
  if (a.length === 3) {
    __rhinoLocalCallback("HttpCallback", {
      authorizationHeader: String(a[0]),
      negotiationHeader: String(a[1]),
      errorCode: a[2],
    });
    return;
  }
  throw new Error(
    "rhino-local: callbacksBuilder.httpCallback arity=" + a.length
  );
};

callbacksBuilder.x509CertificateCallback = function (
  prompt,
  certificate,
  requestSignature
) {
  if (arguments.length < 1 || arguments.length > 3) {
    throw new Error(
      "rhino-local: callbacksBuilder.x509CertificateCallback arity=" +
        arguments.length +
        " (expected 1..3)"
    );
  }
  var x509 = { prompt: String(prompt) };
  if (arguments.length > 1) {
    x509.certificate = certificate;
  }
  if (arguments.length > 2) {
    x509.requestSignature = Boolean(requestSignature);
  }
  __rhinoLocalCallback("X509CertificateCallback", x509);
};

callbacksBuilder.consentMappingCallback = function () {
  var a = arguments;
  if (a.length === 7) {
    __rhinoLocalCallback("ConsentMappingCallback", {
      name: String(a[0]),
      displayName: String(a[1]),
      icon: String(a[2]),
      accessLevel: String(a[3]),
      titles: a[4],
      message: String(a[5]),
      isRequired: Boolean(a[6]),
    });
    return;
  }
  if (a.length === 3) {
    __rhinoLocalCallback("ConsentMappingCallback", {
      config: a[0],
      message: String(a[1]),
      isRequired: Boolean(a[2]),
    });
    return;
  }
  throw new Error(
    "rhino-local: callbacksBuilder.consentMappingCallback arity=" + a.length
  );
};

callbacksBuilder.deviceProfileCallback = function (metadata, location, message) {
  __rhinoLocalExpectArity("callbacksBuilder.deviceProfileCallback", arguments, 3);
  __rhinoLocalCallback("DeviceProfileCallback", {
    metadata: Boolean(metadata),
    location: Boolean(location),
    message: String(message),
  });
};

callbacksBuilder.kbaCreateCallback = function (
  prompt,
  predefinedQuestions,
  allowUserDefinedQuestions
) {
  __rhinoLocalExpectArity("callbacksBuilder.kbaCreateCallback", arguments, 3);
  __rhinoLocalCallback("KbaCreateCallback", {
    prompt: String(prompt),
    predefinedQuestions: predefinedQuestions,
    allowUserDefinedQuestions: Boolean(allowUserDefinedQuestions),
  });
};

callbacksBuilder.selectIdPCallback = function (providers) {
  __rhinoLocalExpectArity("callbacksBuilder.selectIdPCallback", arguments, 1);
  __rhinoLocalCallback("SelectIdPCallback", { providers: providers });
};

callbacksBuilder.termsAndConditionsCallback = function (version, terms, createDate) {
  __rhinoLocalExpectArity(
    "callbacksBuilder.termsAndConditionsCallback",
    arguments,
    3
  );
  __rhinoLocalCallback("TermsAndConditionsCallback", {
    version: String(version),
    terms: String(terms),
    createDate: String(createDate),
  });
};

callbacksBuilder.metadataCallback = function (outputValue) {
  __rhinoLocalExpectArity("callbacksBuilder.metadataCallback", arguments, 1);
  __rhinoLocalCallback("MetadataCallback", { outputValue: outputValue });
};

callbacksBuilder.pollingWaitCallback = function (waitTime, message) {
  __rhinoLocalExpectArity("callbacksBuilder.pollingWaitCallback", arguments, 2);
  __rhinoLocalCallback("PollingWaitCallback", {
    waitTime: String(waitTime),
    message: String(message),
  });
};

callbacksBuilder.redirectCallback = function (
  redirectUrl,
  redirectData,
  method,
  fourth,
  fifth,
  sixth
) {
  var a = arguments;
  var redirectFields = {
    redirectUrl: String(redirectUrl),
    redirectData: redirectData,
    method: String(method),
  };
  if (a.length === 3) {
    __rhinoLocalCallback("RedirectCallback", redirectFields);
    return;
  }
  if (a.length === 4) {
    redirectFields.setTrackingCookie = Boolean(fourth);
    __rhinoLocalCallback("RedirectCallback", redirectFields);
    return;
  }
  if (a.length === 5) {
    redirectFields.statusParameter = String(fourth);
    redirectFields.redirectBackUrlCookie = String(fifth);
    __rhinoLocalCallback("RedirectCallback", redirectFields);
    return;
  }
  if (a.length === 6) {
    redirectFields.statusParameter = String(fourth);
    redirectFields.redirectBackUrlCookie = String(fifth);
    redirectFields.setTrackingCookie = Boolean(sixth);
    __rhinoLocalCallback("RedirectCallback", redirectFields);
    return;
  }
  throw new Error(
    "rhino-local: callbacksBuilder.redirectCallback arity=" + a.length
  );
};

function __rhinoLocalAttributeCallback(type, args) {
  var fields = {
    name: String(args[0]),
    prompt: String(args[1]),
    value: args[2],
    required: Boolean(args[3]),
  };
  if (args.length === 4) {
    __rhinoLocalCallback(type, fields);
    return;
  }
  if (args.length === 5) {
    fields.failedPolicies = args[4];
    __rhinoLocalCallback(type, fields);
    return;
  }
  if (args.length === 6) {
    fields.policies = args[4];
    fields.validateOnly = Boolean(args[5]);
    __rhinoLocalCallback(type, fields);
    return;
  }
  if (args.length === 7) {
    fields.policies = args[4];
    fields.validateOnly = Boolean(args[5]);
    fields.failedPolicies = args[6];
    __rhinoLocalCallback(type, fields);
    return;
  }
  throw new Error(
    "rhino-local: callbacksBuilder." +
      type.charAt(0).toLowerCase() +
      type.substring(1) +
      " arity=" +
      args.length
  );
}

callbacksBuilder.stringAttributeInputCallback = function () {
  __rhinoLocalAttributeCallback("StringAttributeInputCallback", arguments);
};

callbacksBuilder.numberAttributeInputCallback = function () {
  __rhinoLocalAttributeCallback("NumberAttributeInputCallback", arguments);
};

callbacksBuilder.booleanAttributeInputCallback = function () {
  __rhinoLocalAttributeCallback("BooleanAttributeInputCallback", arguments);
};

callbacksBuilder.validatedUsernameCallback = function (
  prompt,
  policies,
  validateOnly,
  failedPolicies
) {
  if (arguments.length !== 3 && arguments.length !== 4) {
    throw new Error(
      "rhino-local: callbacksBuilder.validatedUsernameCallback arity=" +
        arguments.length +
        " (expected 3 or 4)"
    );
  }
  var userFields = {
    prompt: String(prompt),
    policies: policies,
    validateOnly: Boolean(validateOnly),
  };
  if (arguments.length === 4) {
    userFields.failedPolicies = failedPolicies;
  }
  __rhinoLocalCallback("ValidatedUsernameCallback", userFields);
};

callbacksBuilder.validatedPasswordCallback = function (
  prompt,
  echoOn,
  policies,
  validateOnly,
  failedPolicies
) {
  if (arguments.length !== 4 && arguments.length !== 5) {
    throw new Error(
      "rhino-local: callbacksBuilder.validatedPasswordCallback arity=" +
        arguments.length +
        " (expected 4 or 5)"
    );
  }
  var pwFields = {
    prompt: String(prompt),
    echoOn: Boolean(echoOn),
    policies: policies,
    validateOnly: Boolean(validateOnly),
  };
  if (arguments.length === 5) {
    pwFields.failedPolicies = failedPolicies;
  }
  __rhinoLocalCallback("ValidatedPasswordCallback", pwFields);
};

function __rhinoLocalRequireSubmitted() {
  if (__rhinoLocal.submittedCallbacks === null) {
    throw new Error("rhino-local: callbacks: no given.callbacks");
  }
  return __rhinoLocal.submittedCallbacks;
}

function __rhinoLocalSubmittedValues(type) {
  var submitted = __rhinoLocalRequireSubmitted();
  var out = [];
  var i;
  var cb;
  for (i = 0; i < submitted.length; i += 1) {
    cb = submitted[i];
    if (cb.type === type) {
      if (cb.value === undefined) {
        throw new Error(
          "rhino-local: given.callbacks[" +
            i +
            "] (" +
            type +
            ") has no value"
        );
      }
      out.push(cb.value);
    }
  }
  return __rhinoLocalJavaList(out);
}

callbacks.isEmpty = function () {
  __rhinoLocalExpectArity("callbacks.isEmpty", arguments, 0);
  return __rhinoLocalRequireSubmitted().length === 0;
};

callbacks.getNameCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getNameCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("NameCallback");
};

callbacks.getPasswordCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getPasswordCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("PasswordCallback");
};

callbacks.getHiddenValueCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getHiddenValueCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("HiddenValueCallback");
};

callbacks.getDeviceProfileCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getDeviceProfileCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("DeviceProfileCallback");
};

callbacks.getKbaCreateCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getKbaCreateCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("KbaCreateCallback");
};

callbacks.getSelectIdPCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getSelectIdPCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("SelectIdPCallback");
};

callbacks.getTermsAndConditionsCallbacks = function () {
  __rhinoLocalExpectArity(
    "callbacks.getTermsAndConditionsCallbacks",
    arguments,
    0
  );
  return __rhinoLocalSubmittedValues("TermsAndConditionsCallback");
};

callbacks.getTextInputCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getTextInputCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("TextInputCallback");
};

callbacks.getStringAttributeInputCallbacks = function () {
  __rhinoLocalExpectArity(
    "callbacks.getStringAttributeInputCallbacks",
    arguments,
    0
  );
  return __rhinoLocalSubmittedValues("StringAttributeInputCallback");
};

callbacks.getNumberAttributeInputCallbacks = function () {
  __rhinoLocalExpectArity(
    "callbacks.getNumberAttributeInputCallbacks",
    arguments,
    0
  );
  return __rhinoLocalSubmittedValues("NumberAttributeInputCallback");
};

callbacks.getBooleanAttributeInputCallbacks = function () {
  __rhinoLocalExpectArity(
    "callbacks.getBooleanAttributeInputCallbacks",
    arguments,
    0
  );
  return __rhinoLocalSubmittedValues("BooleanAttributeInputCallback");
};

callbacks.getConfirmationCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getConfirmationCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("ConfirmationCallback");
};

callbacks.getLanguageCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getLanguageCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("LanguageCallback");
};

callbacks.getIdpCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getIdpCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("IdPCallback");
};

callbacks.getValidatedPasswordCallbacks = function () {
  __rhinoLocalExpectArity(
    "callbacks.getValidatedPasswordCallbacks",
    arguments,
    0
  );
  return __rhinoLocalSubmittedValues("ValidatedPasswordCallback");
};

callbacks.getValidatedUsernameCallbacks = function () {
  __rhinoLocalExpectArity(
    "callbacks.getValidatedUsernameCallbacks",
    arguments,
    0
  );
  return __rhinoLocalSubmittedValues("ValidatedUsernameCallback");
};

callbacks.getHttpCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getHttpCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("HttpCallback");
};

callbacks.getX509CertificateCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getX509CertificateCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("X509CertificateCallback");
};

callbacks.getConsentMappingCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getConsentMappingCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("ConsentMappingCallback");
};

callbacks.getChoiceCallbacks = function () {
  __rhinoLocalExpectArity("callbacks.getChoiceCallbacks", arguments, 0);
  return __rhinoLocalSubmittedValues("ChoiceCallback");
};

function __rhinoLocalSplitResource(id) {
  var s = String(id);
  var parts = s.split("/");
  if (parts.length >= 3) {
    return {
      collection: parts.slice(0, parts.length - 1).join("/"),
      recordId: parts[parts.length - 1],
    };
  }
  return { collection: s, recordId: null };
}

function __rhinoLocalFindRecord(collection, recordId) {
  var rows = __rhinoLocal.managed[collection];
  if (!rows) {
    return { rows: null, index: -1, record: null };
  }
  var i;
  for (i = 0; i < rows.length; i += 1) {
    if (String(rows[i]._id) === String(recordId)) {
      return { rows: rows, index: i, record: rows[i] };
    }
  }
  return { rows: rows, index: -1, record: null };
}

function __rhinoLocalRequireCollection(method, collection) {
  if (!__rhinoLocalHas(__rhinoLocal.managed, collection)) {
    throw new Error(
      "rhino-local: openidm." +
        method +
        ": no given.managed entry for " +
        JSON.stringify(collection)
    );
  }
  return __rhinoLocal.managed[collection];
}

function __rhinoLocalRequireRecord(method, resource) {
  var split = __rhinoLocalSplitResource(resource);
  if (split.recordId === null) {
    throw new Error(
      "rhino-local: openidm." +
        method +
        ": " +
        JSON.stringify(resource) +
        " is a collection path, not a record"
    );
  }
  __rhinoLocalRequireCollection(method, split.collection);
  var found = __rhinoLocalFindRecord(split.collection, split.recordId);
  if (!found.record) {
    throw new Error(
      "rhino-local: openidm." +
        method +
        ": no given.managed entry for " +
        JSON.stringify(resource)
    );
  }
  return found;
}

function __rhinoLocalProject(record, fields) {
  if (!fields || !Array.isArray(fields) || fields.length === 0) {
    return record;
  }
  var i;
  for (i = 0; i < fields.length; i += 1) {
    if (fields[i] === "*") {
      return record;
    }
  }
  var out = {};
  if (record._id !== undefined) {
    out._id = record._id;
  }
  if (record._rev !== undefined) {
    out._rev = record._rev;
  }
  var field;
  var parent;
  for (i = 0; i < fields.length; i += 1) {
    field = String(fields[i]);
    if (__rhinoLocalHas(record, field)) {
      out[field] = record[field];
    } else if (field.indexOf("/") !== -1) {
      parent = field.split("/")[0];
      if (__rhinoLocalHas(record, parent) && !__rhinoLocalHas(out, parent)) {
        out[parent] = record[parent];
      }
    }
  }
  return out;
}

function __rhinoLocalPushOpenidm(method, resource, body, actionName) {
  var rec = { method: method, resource: resource };
  if (body !== undefined) {
    rec.body = __rhinoLocalClone(body);
  }
  if (actionName !== undefined) {
    rec.actionName = actionName;
  }
  __rhinoLocal.openidm.push(rec);
}

function __rhinoLocalFilterError(filter) {
  throw new Error(
    "rhino-local: openidm.query: unmocked filter " + JSON.stringify(filter)
  );
}

function __rhinoLocalIsIdentChar(ch) {
  return (
    (ch >= "a" && ch <= "z") ||
    (ch >= "A" && ch <= "Z") ||
    (ch >= "0" && ch <= "9") ||
    ch === "_" ||
    ch === "-"
  );
}

function __rhinoLocalReadField(record, pointer) {
  var path = String(pointer);
  if (path.charAt(0) === "/") {
    path = path.substring(1);
  }
  if (path === "") {
    return record;
  }
  var parts = path.split("/");
  var cur = record;
  var i;
  for (i = 0; i < parts.length; i += 1) {
    if (cur === null || cur === undefined || typeof cur !== "object") {
      return undefined;
    }
    if (!__rhinoLocalHas(cur, parts[i])) {
      return undefined;
    }
    cur = cur[parts[i]];
  }
  return cur;
}

function __rhinoLocalCompareOne(actual, op, expected) {
  var left = actual === null || actual === undefined ? "" : String(actual);
  var right = expected === null || expected === undefined ? "" : String(expected);
  if (op === "eq") {
    return left === right;
  }
  if (op === "co") {
    return left.indexOf(right) !== -1;
  }
  if (op === "sw") {
    return left.indexOf(right) === 0;
  }
  return false;
}

function __rhinoLocalCompareField(record, pointer, op, expected) {
  var actual = __rhinoLocalReadField(record, pointer);
  if (op === "pr") {
    return actual !== undefined && actual !== null;
  }
  if (actual === undefined || actual === null) {
    return false;
  }
  if (Array.isArray(actual)) {
    var i;
    for (i = 0; i < actual.length; i += 1) {
      if (__rhinoLocalCompareOne(actual[i], op, expected)) {
        return true;
      }
    }
    return false;
  }
  return __rhinoLocalCompareOne(actual, op, expected);
}

function __rhinoLocalParseFilter(filter) {
  var src = String(filter);
  var i = 0;

  function skipWs() {
    while (i < src.length) {
      var ch = src.charAt(i);
      if (ch !== " " && ch !== "\t" && ch !== "\n" && ch !== "\r") {
        break;
      }
      i += 1;
    }
  }

  function peek() {
    skipWs();
    if (i >= src.length) {
      return "";
    }
    return src.charAt(i);
  }

  function readIdent() {
    skipWs();
    var start = i;
    while (i < src.length && __rhinoLocalIsIdentChar(src.charAt(i))) {
      i += 1;
    }
    if (start === i) {
      return "";
    }
    return src.substring(start, i);
  }

  function readField() {
    skipWs();
    var start = i;
    if (i < src.length && src.charAt(i) === "/") {
      i += 1;
    }
    if (!__rhinoLocalIsIdentChar(peekCharRaw())) {
      i = start;
      return null;
    }
    while (i < src.length) {
      var ch = src.charAt(i);
      if (__rhinoLocalIsIdentChar(ch) || ch === "/") {
        i += 1;
      } else {
        break;
      }
    }
    var field = src.substring(start, i);
    if (field === "" || field === "/") {
      i = start;
      return null;
    }
    return field;
  }

  function peekCharRaw() {
    if (i >= src.length) {
      return "";
    }
    return src.charAt(i);
  }

  function readString() {
    skipWs();
    if (src.charAt(i) !== '"') {
      return null;
    }
    i += 1;
    var out = "";
    while (i < src.length) {
      var ch = src.charAt(i);
      if (ch === "\\") {
        i += 1;
        if (i >= src.length) {
          __rhinoLocalFilterError(filter);
        }
        out += src.charAt(i);
        i += 1;
      } else if (ch === '"') {
        i += 1;
        return out;
      } else {
        out += ch;
        i += 1;
      }
    }
    __rhinoLocalFilterError(filter);
    return null;
  }

  function readNumber() {
    skipWs();
    var start = i;
    if (src.charAt(i) === "-") {
      i += 1;
    }
    var digits = 0;
    while (i < src.length && src.charAt(i) >= "0" && src.charAt(i) <= "9") {
      digits += 1;
      i += 1;
    }
    if (src.charAt(i) === ".") {
      i += 1;
      while (i < src.length && src.charAt(i) >= "0" && src.charAt(i) <= "9") {
        digits += 1;
        i += 1;
      }
    }
    if (digits === 0) {
      i = start;
      return null;
    }
    return Number(src.substring(start, i));
  }

  function parseValue() {
    skipWs();
    var str = readString();
    if (str !== null) {
      return str;
    }
    var ident = "";
    var saved = i;
    ident = readIdent();
    if (ident === "true") {
      return true;
    }
    if (ident === "false") {
      return false;
    }
    if (ident === "null") {
      return null;
    }
    i = saved;
    var num = readNumber();
    if (num !== null && !isNaN(num)) {
      return num;
    }
    __rhinoLocalFilterError(filter);
    return null;
  }

  function parsePrimary() {
    skipWs();
    if (peek() === "(") {
      i += 1;
      var inner = parseOr();
      skipWs();
      if (peek() !== ")") {
        __rhinoLocalFilterError(filter);
      }
      i += 1;
      return inner;
    }
    var saved = i;
    var ident = readIdent();
    if (ident === "true") {
      return { kind: "true" };
    }
    if (ident === "false") {
      return { kind: "false" };
    }
    i = saved;
    var field = readField();
    if (field === null) {
      __rhinoLocalFilterError(filter);
    }
    skipWs();
    var op = readIdent();
    if (op === "pr") {
      return { kind: "pr", field: field };
    }
    if (op === "eq" || op === "co" || op === "sw") {
      return { kind: "cmp", field: field, op: op, value: parseValue() };
    }
    __rhinoLocalFilterError(filter);
    return null;
  }

  function parseNot() {
    skipWs();
    if (peek() === "!") {
      i += 1;
      return { kind: "not", inner: parseNot() };
    }
    var saved = i;
    var ident = readIdent();
    if (ident === "not") {
      // AIC rejects the word form (docs/api/10, verified 2026-07-03).
      __rhinoLocalFilterError(filter);
    }
    i = saved;
    return parsePrimary();
  }

  function parseAnd() {
    var left = parseNot();
    while (true) {
      var saved = i;
      var ident = readIdent();
      if (ident !== "and") {
        i = saved;
        break;
      }
      left = { kind: "and", left: left, right: parseNot() };
    }
    return left;
  }

  function parseOr() {
    var left = parseAnd();
    while (true) {
      var saved = i;
      var ident = readIdent();
      if (ident !== "or") {
        i = saved;
        break;
      }
      left = { kind: "or", left: left, right: parseAnd() };
    }
    return left;
  }

  var ast = parseOr();
  skipWs();
  if (i !== src.length) {
    __rhinoLocalFilterError(filter);
  }
  return ast;
}

function __rhinoLocalEvalFilter(record, ast) {
  if (ast.kind === "true") {
    return true;
  }
  if (ast.kind === "false") {
    return false;
  }
  if (ast.kind === "not") {
    return !__rhinoLocalEvalFilter(record, ast.inner);
  }
  if (ast.kind === "and") {
    return (
      __rhinoLocalEvalFilter(record, ast.left) &&
      __rhinoLocalEvalFilter(record, ast.right)
    );
  }
  if (ast.kind === "or") {
    return (
      __rhinoLocalEvalFilter(record, ast.left) ||
      __rhinoLocalEvalFilter(record, ast.right)
    );
  }
  if (ast.kind === "pr") {
    return __rhinoLocalCompareField(record, ast.field, "pr", null);
  }
  if (ast.kind === "cmp") {
    return __rhinoLocalCompareField(record, ast.field, ast.op, ast.value);
  }
  return false;
}

function __rhinoLocalMatchesFilter(record, filter) {
  if (!filter || filter === "true") {
    return true;
  }
  if (filter === "false") {
    return false;
  }
  return __rhinoLocalEvalFilter(record, __rhinoLocalParseFilter(filter));
}

openidm.read = function (resourceName, _params, fields) {
  if (arguments.length < 1 || arguments.length > 3) {
    throw new Error(
      "rhino-local: openidm.read arity=" + arguments.length + " (expected 1..3)"
    );
  }
  var resource = String(resourceName);
  __rhinoLocalPushOpenidm("read", resource);
  var split = __rhinoLocalSplitResource(resource);
  if (split.recordId === null) {
    throw new Error(
      "rhino-local: openidm.read: " +
        JSON.stringify(resource) +
        " is a collection path, not a record"
    );
  }
  __rhinoLocalRequireCollection("read", split.collection);
  var found = __rhinoLocalFindRecord(split.collection, split.recordId);
  if (!found.record) {
    // AIC returns null for a missing record in a known collection
    // (docs/api/10, verified 2026-07-17). An unseeded collection still
    // throws above — that is a missing given.managed fixture.
    return null;
  }
  return __rhinoLocalProject(found.record, fields);
};

openidm.query = function (resourceName, params, fields) {
  if (arguments.length < 2 || arguments.length > 3) {
    throw new Error(
      "rhino-local: openidm.query arity=" + arguments.length + " (expected 2 or 3)"
    );
  }
  var collection = String(resourceName);
  __rhinoLocalPushOpenidm("query", collection, params);
  var rows = __rhinoLocalRequireCollection("query", collection);
  var filter = params && params._queryFilter;
  var result = [];
  var i;
  for (i = 0; i < rows.length; i += 1) {
    if (__rhinoLocalMatchesFilter(rows[i], filter)) {
      result.push(__rhinoLocalProject(rows[i], fields));
    }
  }
  return {
    result: result,
    resultCount: result.length,
    pagedResultsCookie: null,
    totalPagedResultsPolicy: "NONE",
    totalPagedResults: -1,
  };
};

openidm.create = function (resourceName, newResourceId, content) {
  if (arguments.length < 3 || arguments.length > 5) {
    throw new Error(
      "rhino-local: openidm.create arity=" + arguments.length + " (expected 3..5)"
    );
  }
  var collection = String(resourceName);
  var id = newResourceId;
  var recordedResource;
  if (id === null || id === undefined || id === "") {
    __rhinoLocal.generatedId += 1;
    id = "generated-" + __rhinoLocal.generatedId;
    recordedResource = collection;
  } else {
    id = String(id);
    recordedResource = collection + "/" + id;
  }
  __rhinoLocalPushOpenidm("create", recordedResource, content);
  if (!__rhinoLocalHas(__rhinoLocal.managed, collection)) {
    __rhinoLocal.managed[collection] = [];
  }
  var stored = __rhinoLocalClone(content);
  stored._id = id;
  if (stored._rev === undefined) {
    stored._rev = "0";
  }
  __rhinoLocal.managed[collection].push(stored);
  return stored;
};

openidm.update = function (id, _rev, value) {
  if (arguments.length < 3 || arguments.length > 5) {
    throw new Error(
      "rhino-local: openidm.update arity=" + arguments.length + " (expected 3..5)"
    );
  }
  var resource = String(id);
  __rhinoLocalPushOpenidm("update", resource, value);
  var found = __rhinoLocalRequireRecord("update", resource);
  var next = __rhinoLocalClone(value);
  next._id = found.record._id;
  if (next._rev === undefined) {
    next._rev = found.record._rev;
  }
  found.rows[found.index] = next;
  return next;
};

openidm.patch = function (resourceName, _rev, patch) {
  if (arguments.length < 3 || arguments.length > 5) {
    throw new Error(
      "rhino-local: openidm.patch arity=" + arguments.length + " (expected 3..5)"
    );
  }
  var resource = String(resourceName);
  __rhinoLocalPushOpenidm("patch", resource, patch);
  var found = __rhinoLocalRequireRecord("patch", resource);
  var i;
  var op;
  var field;
  for (i = 0; i < patch.length; i += 1) {
    op = patch[i];
    field = String(op.field).replace(/^\//, "");
    if (op.operation === "remove") {
      delete found.record[field];
    } else {
      found.record[field] = op.value;
    }
  }
  return found.record;
};

openidm.delete = function (resourceName) {
  if (arguments.length < 2 || arguments.length > 4) {
    throw new Error(
      "rhino-local: openidm.delete arity=" + arguments.length + " (expected 2..4)"
    );
  }
  var resource = String(resourceName);
  __rhinoLocalPushOpenidm("delete", resource);
  var found = __rhinoLocalRequireRecord("delete", resource);
  var removed = found.record;
  found.rows.splice(found.index, 1);
  return removed;
};

openidm.action = function (resource, actionName, content, params) {
  if (arguments.length < 2 || arguments.length > 5) {
    throw new Error(
      "rhino-local: openidm.action arity=" + arguments.length + " (expected 2..5)"
    );
  }
  var body = content;
  if (body === undefined || body === null) {
    body = params;
  }
  __rhinoLocalPushOpenidm("action", String(resource), body, String(actionName));
  return {};
};

function __rhinoLocalAsPattern(value) {
  if (value && typeof value === "object" && typeof value.__regex === "string") {
    return new RegExp(value.__regex, value.__flags || "");
  }
  return value;
}

function __rhinoLocalMatchUrl(pattern, url) {
  var p = __rhinoLocalAsPattern(pattern);
  if (typeof p === "string") {
    return p === url;
  }
  if (p && typeof p.test === "function") {
    return p.test(url);
  }
  throw new Error("rhino-local: given.http match.url is not a string or regexp");
}

function __rhinoLocalMatchHttp(url, method) {
  var i;
  var stub;
  var wantMethod;
  for (i = 0; i < __rhinoLocal.httpStubs.length; i += 1) {
    stub = __rhinoLocal.httpStubs[i];
    if (!__rhinoLocalMatchUrl(stub.match.url, url)) {
      continue;
    }
    wantMethod = stub.match.method;
    if (wantMethod && String(wantMethod).toUpperCase() !== method) {
      continue;
    }
    return stub.reply;
  }
  return null;
}

httpClient.send = function (uri, requestOptions) {
  if (arguments.length < 1 || arguments.length > 2) {
    throw new Error(
      "rhino-local: httpClient.send arity=" + arguments.length + " (expected 1 or 2)"
    );
  }
  var url = String(uri);
  var method = "GET";
  var body;
  if (requestOptions && requestOptions.method) {
    method = String(requestOptions.method).toUpperCase();
  }
  if (requestOptions && requestOptions.body !== undefined) {
    body = requestOptions.body;
  }
  var effect = { url: url, method: method };
  if (body !== undefined) {
    effect.body = body;
  }
  __rhinoLocal.http.push(effect);
  var reply = __rhinoLocalMatchHttp(url, method);
  if (!reply) {
    throw new Error(
      "rhino-local: httpClient.send: no given.http stub for " + method + " " + url
    );
  }
  var response = {
    status: reply.status,
    ok: reply.status >= 200 && reply.status < 300,
    statusText: String(reply.status),
    headers: reply.headers || {},
    json: function () {
      return reply.body;
    },
    text: function () {
      if (typeof reply.body === "string") {
        return reply.body;
      }
      if (reply.body === undefined) {
        return "";
      }
      return JSON.stringify(reply.body);
    },
  };
  return {
    get: function () {
      return response;
    },
  };
};

idRepository.getIdentity = function (userName) {
  __rhinoLocalExpectArity("idRepository.getIdentity", arguments, 1);
  var wanted = String(userName);
  var collections = Object.keys(__rhinoLocal.managed);
  var c;
  var rows;
  var i;
  var record;
  var found = null;
  for (c = 0; c < collections.length; c += 1) {
    rows = __rhinoLocal.managed[collections[c]];
    for (i = 0; i < rows.length; i += 1) {
      record = rows[i];
      if (String(record._id) === wanted || String(record.userName) === wanted) {
        found = record;
        break;
      }
    }
    if (found) {
      break;
    }
  }
  if (!found) {
    throw new Error(
      "rhino-local: idRepository.getIdentity: no given.managed record for " +
        JSON.stringify(wanted)
    );
  }
  return {
    getName: function () {
      if (found.userName !== undefined) {
        return found.userName;
      }
      return found._id;
    },
    getUniversalId: function () {
      return found._id;
    },
    exists: function () {
      return true;
    },
    getAttributeValues: function (attributeName) {
      var value = found[attributeName];
      if (value === undefined || value === null) {
        return __rhinoLocalJavaList([]);
      }
      if (Array.isArray(value)) {
        return __rhinoLocalJavaList(value);
      }
      return __rhinoLocalJavaList([value]);
    },
    setAttribute: function () {
      __rhinoLocalNotMocked("idRepository.getIdentity()", "setAttribute", arguments, [
        {
          arity: 2,
          types: ["string", "array"],
          label: "setAttribute(attributeName: string, attributeValues: array)",
        },
      ]);
    },
    addAttribute: function () {
      __rhinoLocalNotMocked("idRepository.getIdentity()", "addAttribute", arguments, [
        {
          arity: 2,
          types: ["string", "string"],
          label: "addAttribute(attributeName: string, attributeValue: string)",
        },
      ]);
    },
    store: function () {
      __rhinoLocalNotMocked("idRepository.getIdentity()", "store", arguments, [
        { arity: 0, types: [], label: "store()" },
      ]);
    },
  };
};

systemEnv = {
  getProperty: function (key) {
    __rhinoLocalExpectArity("systemEnv.getProperty", arguments, 1);
    var k = String(key);
    if (!__rhinoLocalHas(__rhinoLocal.esv, k)) {
      throw new Error(
        "rhino-local: systemEnv.getProperty: no given.esv entry for " +
          JSON.stringify(k)
      );
    }
    return __rhinoLocal.esv[k];
  },
};

function __rhinoLocalSecret(value) {
  return {
    getAsUtf8: function () {
      __rhinoLocalExpectArity("secret.getAsUtf8", arguments, 0);
      return value;
    },
  };
}

function __rhinoLocalRequireSecret(method, secretId) {
  var id = String(secretId);
  if (!__rhinoLocalHas(__rhinoLocal.secrets, id)) {
    throw new Error(
      "rhino-local: secrets." +
        method +
        ": no given.secrets entry for " +
        JSON.stringify(id)
    );
  }
  return __rhinoLocalSecret(__rhinoLocal.secrets[id]);
}

secrets.getGenericSecret = function (secretId) {
  __rhinoLocalExpectArity("secrets.getGenericSecret", arguments, 1);
  return __rhinoLocalRequireSecret("getGenericSecret", secretId);
};

secrets.getDecryptionKey = function (secretId) {
  __rhinoLocalExpectArity("secrets.getDecryptionKey", arguments, 1);
  return __rhinoLocalRequireSecret("getDecryptionKey", secretId);
};

secrets.getEncryptionKey = function (secretId) {
  __rhinoLocalExpectArity("secrets.getEncryptionKey", arguments, 1);
  return __rhinoLocalRequireSecret("getEncryptionKey", secretId);
};

secrets.getSigningKey = function (secretId) {
  __rhinoLocalExpectArity("secrets.getSigningKey", arguments, 1);
  return __rhinoLocalRequireSecret("getSigningKey", secretId);
};

secrets.getVerificationKey = function (secretId) {
  __rhinoLocalExpectArity("secrets.getVerificationKey", arguments, 1);
  return __rhinoLocalRequireSecret("getVerificationKey", secretId);
};

// Capture the Rhino builtin before we shadow it. A function declaration
// would hoist and capture the wrapper itself.
var __rhinoLocalRealJavaImporter =
  typeof JavaImporter === "function" ? JavaImporter : null;

function __rhinoLocalHiddenValueCallback(id, value) {
  this.type = "HiddenValueCallback";
  this.id = String(id);
  this.value = value === undefined || value === null ? "" : String(value);
}

function __rhinoLocalActionBuilder() {
  return {
    build: function () {
      if (__rhinoLocal.outcome === null) {
        __rhinoLocal.outcome = "ok";
        outcome = "ok";
      }
      return action;
    },
  };
}

var __rhinoLocalActionClass = {
  send: function () {
    var i;
    var cb;
    var fields;
    var key;
    for (i = 0; i < arguments.length; i += 1) {
      cb = arguments[i];
      if (!cb) {
        continue;
      }
      fields = {};
      for (key in cb) {
        if (Object.prototype.hasOwnProperty.call(cb, key) && key !== "type") {
          fields[key] = cb[key];
        }
      }
      __rhinoLocalCallback(
        cb.type ? String(cb.type) : "HiddenValueCallback",
        fields
      );
    }
    if (__rhinoLocal.outcome === null) {
      __rhinoLocal.outcome = "ok";
      outcome = "ok";
    }
    return __rhinoLocalActionBuilder();
  },
  goTo: function (name) {
    __rhinoLocal.outcome = String(name);
    outcome = __rhinoLocal.outcome;
    return __rhinoLocalActionBuilder();
  },
};

JavaImporter = function () {
  var real = null;
  if (typeof __rhinoLocalRealJavaImporter === "function") {
    real = __rhinoLocalRealJavaImporter.apply(null, arguments);
  }
  function Importer() {}
  if (real) {
    Importer.prototype = real;
  }
  var wrapper = new Importer();
  wrapper.Action = __rhinoLocalActionClass;
  wrapper.HiddenValueCallback = __rhinoLocalHiddenValueCallback;
  return wrapper;
};

function __rhinoLocalLoadLibrary(name) {
  var id = String(name);
  if (__rhinoLocalHas(__rhinoLocal.requireCache, id)) {
    return __rhinoLocal.requireCache[id];
  }
  if (!__rhinoLocalHas(__rhinoLocal.libraries, id)) {
    throw new Error(
      "rhino-local: require: no given.libraries entry for " + JSON.stringify(id)
    );
  }
  var exports = {};
  var module = { exports: exports };
  __rhinoLocal.requireCache[id] = exports;
  var source = String(__rhinoLocal.libraries[id]);
  var loader = eval(
    "(function (require, exports, module) {\n" + source + "\n})"
  );
  loader(require, exports, module);
  __rhinoLocal.requireCache[id] = module.exports;
  return module.exports;
}

function __rhinoLocalSeed(given) {
  given = given || {};
  if (given.bindings) {
    throw new Error(
      "rhino-local: given.bindings seeding is not implemented; use a dedicated given.* field"
    );
  }
  __rhinoLocal.shared = __rhinoLocalClone(given.sharedState || {});
  __rhinoLocal.transient = __rhinoLocalClone(given.transientState || {});
  __rhinoLocal.secure = __rhinoLocalClone(given.secureState || {});
  __rhinoLocal.initialShared = __rhinoLocalClone(__rhinoLocal.shared);
  __rhinoLocal.initialTransient = __rhinoLocalClone(__rhinoLocal.transient);
  __rhinoLocal.initialSecure = __rhinoLocalClone(__rhinoLocal.secure);
  __rhinoLocal.managed = __rhinoLocalClone(given.managed || {});
  __rhinoLocal.esv = __rhinoLocalClone(given.esv || {});
  __rhinoLocal.secrets = __rhinoLocalClone(given.secrets || {});
  __rhinoLocal.libraries = given.libraries
    ? __rhinoLocalClone(given.libraries)
    : {};
  __rhinoLocal.requireCache = {};
  if (given.engine === "legacy") {
    require = undefined;
  } else {
    require = function (name) {
      __rhinoLocalExpectArity("require", arguments, 1);
      return __rhinoLocalLoadLibrary(name);
    };
  }
  if (given.callbacks === undefined) {
    __rhinoLocal.submittedCallbacks = null;
  } else {
    __rhinoLocal.submittedCallbacks = __rhinoLocalClone(given.callbacks);
  }
  __rhinoLocal.httpStubs = given.http ? __rhinoLocalClone(given.http) : [];
  __rhinoLocal.generatedId = 0;
  __rhinoLocal.outcome = null;
  __rhinoLocal.callbacks = [];
  __rhinoLocal.openidm = [];
  __rhinoLocal.http = [];
  __rhinoLocal.logs = [];

  requestHeaders = __rhinoLocalRequestMap(given.requestHeaders || {}, {
    caseInsensitive: true,
    asList: true,
  });
  requestParameters = __rhinoLocalRequestMap(given.requestParameters || {}, {
    caseInsensitive: false,
    asList: true,
  });
  requestCookies = __rhinoLocalRequestMap(given.requestCookies || {}, {
    caseInsensitive: false,
    asList: false,
  });

  if (given.realm !== undefined) {
    realm = given.realm;
  }
  if (given.scriptName !== undefined) {
    scriptName = given.scriptName;
  }
  if (given.cookieName !== undefined) {
    cookieName = given.cookieName;
  }
  if (given.resumedFromSuspend !== undefined) {
    resumedFromSuspend = given.resumedFromSuspend;
  }
  if (given.locales !== undefined) {
    locales = given.locales;
  }

  if (given.engine === "legacy") {
    sharedState = __rhinoLocalStateMap("shared");
    transientState = __rhinoLocalStateMap("transient");
    // Next-gen-only bindings. Live legacy typeofs them as undefined
    // (docs/api/12); leaving them installed makes scripts take the
    // callbacksBuilder fallback and never hit Action.send.
    callbacksBuilder = undefined;
    action = undefined;
    openidm = undefined;
    utils = undefined;
    requestCookies = undefined;
  }
}

function __rhinoLocalHarvest() {
  var recordedOutcome = __rhinoLocal.outcome;
  if (
    recordedOutcome === null &&
    typeof outcome !== "undefined" &&
    outcome !== null
  ) {
    recordedOutcome = String(outcome);
  }
  return JSON.stringify({
    outcome: recordedOutcome,
    sharedState: {
      initial: __rhinoLocal.initialShared,
      final: __rhinoLocalClone(__rhinoLocal.shared),
    },
    transientState: {
      initial: __rhinoLocal.initialTransient,
      final: __rhinoLocalClone(__rhinoLocal.transient),
    },
    secureState: {
      initial: __rhinoLocal.initialSecure,
      final: __rhinoLocalClone(__rhinoLocal.secure),
    },
    callbacks: __rhinoLocal.callbacks,
    openidm: __rhinoLocal.openidm,
    http: __rhinoLocal.http,
    logs: __rhinoLocal.logs,
  });
}
