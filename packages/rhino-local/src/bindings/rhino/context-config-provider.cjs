// Overlay for AM's CONFIG_PROVIDER_NODE context (legacy and next-gen): the
// Configuration Provider node's script, which builds the configuration of the
// node it wraps at journey runtime. Appended to the shared prelude for a case
// whose `context` names it, after context-common.cjs. Same Rhino 1.7.14
// restrictions as runtime.cjs: no let, no top-level const, no shorthand, no
// Map/Set/Symbol.
//
// Every behaviour here was measured live on 2026-11-10 against a sandbox
// tenant, by running a journey (a decision node seeding the node state, a
// ConfigProviderNode wrapping a MessageNode, a decision node reading back) on
// both engines (docs/api/12-script-bindings-matrix.md, "Config provider
// node"). Anything the measurement did not reach is a named harness fault
// (`config-provider-unmeasured/<binding>.<member>`), never a guess; see
// docs/rhino-local-gaps.md.
//
// AM reads the script's `config` global after the script ends; a script that
// only evaluates to a map is refused. The harvest is appended to the author's
// source, like the access-token context's. The node runs the script again on
// every request that reaches it (callback submission included); one case is
// one invocation.

var __rhinoLocalCfg = {
  seed: null,
};

// Globals the context does not bind (typeof "undefined" on AIC, measured).
// existingSession is in the next-gen descriptor but is a ReferenceError on a
// request without a session cookie (measured; with one, unmeasured). `config` is unbound before the script (measured: a
// ReferenceError), whatever a previous case left.
var __rhinoLocalCfgUnbound = {
  both: [
    "config", "sharedState", "transientState", "outcome", "existingSession", "identity", "session",
    "auditEntryDetail", "properties", "cacheManager",
  ],
  legacy: [
    "callbacks", "callbacksBuilder", "action", "requestCookies", "openidm", "utils", "emailService", "jwtAssertion",
    "jwtValidator", "policy", "cookieName", "journey", "oauthApplication", "samlApplication", "locales",
    "resumedFromSuspend", "require",
  ],
  "next-gen": [],
};

// Every callbacksBuilder method runtime.cjs knows; only textOutputCallback
// was measured here.
var __rhinoLocalCfgBuilderMethods = [
  "booleanAttributeInputCallback", "choiceCallback", "confirmationCallback", "consentMappingCallback",
  "deviceProfileCallback", "hiddenValueCallback", "httpCallback", "idPCallback", "kbaCreateCallback",
  "languageCallback", "metadataCallback", "nameCallback", "numberAttributeInputCallback", "passwordCallback",
  "pollingWaitCallback", "radioChoiceCallback", "redirectCallback", "scriptTextOutputCallback", "selectIdPCallback",
  "stringAttributeInputCallback", "suspendedTextOutputCallback", "termsAndConditionsCallback", "textInputCallback",
  "validatedPasswordCallback", "validatedUsernameCallback", "x509CertificateCallback",
];

var __rhinoLocalCfgCallbackMethods = [
  "getBooleanAttributeInputCallbacks", "getChoiceCallbacks", "getConfirmationCallbacks",
  "getConsentMappingCallbacks", "getDeviceProfileCallbacks", "getHiddenValueCallbacks", "getHttpCallbacks",
  "getIdpCallbacks", "getKbaCreateCallbacks", "getLanguageCallbacks", "getNameCallbacks",
  "getNumberAttributeInputCallbacks", "getPasswordCallbacks", "getSelectIdPCallbacks",
  "getStringAttributeInputCallbacks", "getTermsAndConditionsCallbacks", "getTextInputCallbacks",
  "getValidatedPasswordCallbacks", "getValidatedUsernameCallbacks", "getX509CertificateCallbacks",
];

var __rhinoLocalCfgActionMethods = [
  "withErrorMessage", "withHeader", "withIdentifiedUser", "withIdentifiedAgent", "withStage", "withDescription",
  "withLockoutMessage", "putSessionProperty", "removeSessionProperty", "withMaxSessionTime", "withMaxIdleTime",
  "suspend",
];

__rhinoLocalListKinds.configProvider = {
  id: "config-provider-unmeasured/list",
  noun: "a config provider node list",
  measured: "only length and String(v) were measured",
  outOfRange: false,
  concatReceiver: false,
  concatArgument: false,
  json: false,
  genericReceiver: false,
};

__rhinoLocalCtx.prefix = "config-provider-unmeasured";
__rhinoLocalCtx.noun = "config provider node";
__rhinoLocalCtx.listKind = __rhinoLocalListKinds.configProvider;

// --- Request maps (both engines) ----------------------------------------------

// Measured on both engines: requestHeaders.get(name) and
// requestParameters.get(name) are `[object JavaList]` whose String(v) is
// `[v1]` and whose length is the value count; get of an absent name is null.
// runtime.cjs's decision-node lists differ (a JS array on legacy, an untagged
// view on next-gen), so get is replaced here; the rest of each map is
// runtime.cjs's.
function __rhinoLocalCfgRequestGet(map, values, caseInsensitive, label) {
  var lists = {};
  Object.keys(values || {}).forEach(function (raw) {
    var key = caseInsensitive ? raw.toLowerCase() : raw;
    var items = typeof values[raw] === "string" ? [values[raw]] : values[raw].slice();
    var itemLabel = label + ".get(" + JSON.stringify(key) + ")";
    var list = __rhinoLocalCtxList(items.map(__rhinoLocalCtxString), itemLabel);
    __rhinoLocalCtxTag(list, "JavaList", itemLabel);
    __rhinoLocalCtxRewatch(list);
    lists[key] = list;
  });
  __rhinoLocalHide(map, "get", function (name) {
    if (arguments.length !== 1) {
      throw __rhinoLocalCtxUnmeasured(label + ".get(" + __rhinoLocalArgShape(arguments) + ")");
    }
    var key = caseInsensitive ? String(name).toLowerCase() : String(name);
    return Object.prototype.hasOwnProperty.call(lists, key) ? lists[key] : null;
  });
}

// --- Next-gen: the bindings whose behaviour belongs to the wrapped node -------

// Measured: on the request that reaches the node, callbacks.isEmpty() is
// true. On the re-run when the wrapped node's callbacks are submitted it is
// false; that second invocation is not emulated, and nothing else on the
// binding was read.
function __rhinoLocalCfgCallbacks() {
  var out = __rhinoLocalCtxBare();
  __rhinoLocalCtxMethod(out, "callbacks", "ScriptedCallbacksWrapper", "isEmpty", 0, function () {
    return true;
  });
  __rhinoLocalCtxFaulting(out, "callbacks", __rhinoLocalCfgCallbackMethods);
  __rhinoLocalCtxToJSON(out, "callbacks");
  __rhinoLocalCtxTag(out, null, "callbacks");
  return __rhinoLocalCtxWatch(out, "callbacks");
}

// Measured: callbacksBuilder.textOutputCallback(0, "...") throws nothing and
// adds no callback to what the wrapped node shows. Every other method is
// unmeasured here.
function __rhinoLocalCfgCallbacksBuilder() {
  var out = __rhinoLocalCtxBare();
  __rhinoLocalCtxMethod(out, "callbacksBuilder", "CallbacksBuilder", "textOutputCallback", 2, function () {
    return undefined;
  });
  __rhinoLocalCtxFaulting(out, "callbacksBuilder", __rhinoLocalCfgBuilderMethods);
  __rhinoLocalCtxToJSON(out, "callbacksBuilder");
  __rhinoLocalCtxTag(out, null, "callbacksBuilder");
  return __rhinoLocalCtxWatch(out, "callbacksBuilder");
}

// Measured: action.goTo("false") throws nothing; the journey routes on the
// wrapped node's outcome either way. goTo answers the action, as the
// decision node's ActionWrapper does. Every other member is unmeasured here.
function __rhinoLocalCfgAction() {
  var out = __rhinoLocalCtxBare();
  __rhinoLocalCtxMethod(out, "action", "ActionWrapper", "goTo", 1, function () {
    return out;
  });
  __rhinoLocalCtxFaulting(out, "action", __rhinoLocalCfgActionMethods);
  __rhinoLocalCtxToJSON(out, "action");
  __rhinoLocalCtxTag(out, null, "action");
  return __rhinoLocalCtxWatch(out, "action");
}

// --- Legacy: nodeState.get answers a JsonValue -------------------------------

// Measured on strings, numbers, a nested object, a boolean and a transient
// value: `[object JavaObject]`, String(v) the JSON rendering with doubles
// (`"hello"`, `42.0`, `{ "a": 1.0, ... }`), `.a` undefined and
// JSON.stringify(v) that rendering as a JSON string. A key the state does
// not hold answers JS null.
function __rhinoLocalCfgLegacyGet(nodeGet) {
  return function (key) {
    var value = nodeGet.apply(nodeState, arguments);
    var label = "nodeState.get(" + JSON.stringify(String(key)) + ")";
    if (value === null) {
      if (nodeState.isDefined(key)) {
        throw __rhinoLocalCtxUnmeasured("nodeState.get()", "of a key holding null");
      }
      return null;
    }
    var out = __rhinoLocalCtxJsonValue(value);
    __rhinoLocalHide(out, "toJSON", function () {
      return String(out);
    });
    return __rhinoLocalCtxWatch(out, label);
  };
}

// --- Seed ----------------------------------------------------------------------

// Runs after __rhinoLocalSeed, on the same seed object.
function __rhinoLocalSeedConfigProvider(given) {
  given = given || {};
  __rhinoLocalCtxBegin(given.engine);
  var engine = __rhinoLocalCtx.engine;
  var legacy = engine === "legacy";
  // logger.getName() is a java.lang.String on legacy (measured).
  __rhinoLocalCtx.realJavaStrings = legacy;
  __rhinoLocalCtxUnbind(__rhinoLocalCfgUnbound.both.concat(__rhinoLocalCfgUnbound[engine]));
  __rhinoLocalCfg.seed = { engine: engine };
  if (given.realm === undefined) {
    realm = "/alpha";
  }
  var loggerName = "scripts.CONFIG_PROVIDER_NODE" + (legacy ? "" : "_NEXT_GEN") + "." +
    __rhinoLocal.loggerScriptId + ".(" + scriptName + ")";
  __rhinoLocalHide(logger, "getName", function () {
    return legacy ? __rhinoLocalCtxJavaString(loggerName) : loggerName;
  });
  __rhinoLocalCfgRequestGet(requestHeaders, given.requestHeaders, true, "requestHeaders");
  __rhinoLocalCfgRequestGet(requestParameters, given.requestParameters, false, "requestParameters");
  if (legacy) {
    __rhinoLocalHide(nodeState, "get", __rhinoLocalCfgLegacyGet(nodeState.get));
    // nodeState.keys() is runtime.cjs's: measured live on this engine (a key
    // put with putShared earlier in the same script is listed, and so are
    // transient keys), unlike the social context's start-of-script snapshot.
  } else {
    // Measured: the context's class shutter refuses the Java set keys() builds.
    __rhinoLocalHide(nodeState, "keys", function () {
      throw __rhinoLocalShutterError("java.util.HashSet");
    });
    callbacks = __rhinoLocalCfgCallbacks();
    callbacksBuilder = __rhinoLocalCfgCallbacksBuilder();
    action = __rhinoLocalCfgAction();
  }
  __rhinoLocalCtxSeal();
}

// --- Harvest ---------------------------------------------------------------------

function __rhinoLocalCfgIsJava(value) {
  return typeof java !== "undefined" && value !== null && typeof value === "object" &&
    value instanceof java.lang.Object;
}

// One value of a legacy java.util.Map assigned to `config`. Measured: a
// HashMap holding a string and a nested HashMap of a string.
function __rhinoLocalCfgFromJava(value, path) {
  var out;
  var it;
  var entry;
  if (value instanceof java.lang.String) {
    return String(value);
  }
  if (typeof value === "string") {
    return value;
  }
  if (value instanceof java.util.Map) {
    out = {};
    it = value.entrySet().iterator();
    while (it.hasNext()) {
      entry = it.next();
      if (!(entry.getKey() instanceof java.lang.String) && typeof entry.getKey() !== "string") {
        throw __rhinoLocalCtxUnmeasured("config" + path, "(a Java map with a non-string key)");
      }
      Object.defineProperty(out, String(entry.getKey()), {
        value: __rhinoLocalCfgFromJava(entry.getValue(), path + "." + entry.getKey()),
        enumerable: true, configurable: true, writable: true,
      });
    }
    return out;
  }
  throw __rhinoLocalCtxUnmeasured("config" + path, "(a value in a Java map that is not a string or Java map; " +
    "only those were measured)");
}

// One value inside a plain-object `config`. Measured: strings, integral and
// fractional numbers (AM keeps them as doubles), booleans, null and undefined
// (becomes null) at depths one and two, an array of a number and a string,
// and a nested object. A null or undefined inside an array is unmeasured.
function __rhinoLocalCfgFromJs(value, path, inArray) {
  var out;
  var keys;
  var i;
  if (value === null || value === undefined) {
    if (inArray) {
      throw __rhinoLocalCtxUnmeasured("config" + path, "(a null or undefined inside an array)");
    }
    return null;
  }
  if (typeof value === "string" || typeof value === "boolean") {
    return value;
  }
  if (typeof value === "number") {
    if (!isFinite(value)) {
      throw __rhinoLocalCtxUnmeasured("config" + path, "(a non-finite number)");
    }
    return value;
  }
  if (Array.isArray(value)) {
    out = [];
    for (i = 0; i < value.length; i += 1) {
      out.push(__rhinoLocalCfgFromJs(value[i], path + "[" + i + "]", true));
    }
    return out;
  }
  if (__rhinoLocalCtxIsJsonObject(value) && !__rhinoLocalCfgIsJava(value)) {
    out = {};
    keys = Object.keys(value);
    for (i = 0; i < keys.length; i += 1) {
      Object.defineProperty(out, keys[i], {
        value: __rhinoLocalCfgFromJs(value[keys[i]], path + "." + keys[i], false),
        enumerable: true, configurable: true, writable: true,
      });
    }
    return out;
  }
  throw __rhinoLocalCtxUnmeasured("config" + path, "(" + __rhinoLocalArgShape([value]) +
    "; only strings, finite numbers, booleans, null, undefined, arrays and plain objects were measured)");
}

function __rhinoLocalCfgResult(assigned, value) {
  var refused = function (what) {
    return {
      returned: "none",
      config: null,
      refusal: "config is " + what + "; AM fails the journey with \"Script must set 'config' to a Map.\" (HTTP 500)",
    };
  };
  if (!assigned) {
    return refused("never assigned");
  }
  if (value === undefined) {
    return refused("undefined");
  }
  if (value === null) {
    return refused("null");
  }
  if (typeof value === "string") {
    return refused("a string");
  }
  if (typeof value === "number") {
    return refused("a number");
  }
  if (typeof value === "boolean") {
    return refused("a boolean");
  }
  if (typeof value === "function") {
    return refused("a function");
  }
  if (Array.isArray(value)) {
    return refused("an array");
  }
  var config;
  if (__rhinoLocalCfgIsJava(value)) {
    if (__rhinoLocalCtx.engine === "legacy" && value instanceof java.util.Map) {
      config = __rhinoLocalCfgFromJava(value, "");
    } else {
      throw __rhinoLocalCtxUnmeasured("config", "(" + __rhinoLocalArgShape([value]) +
        "; only a java.util.HashMap on the legacy engine was measured)");
    }
  } else if (__rhinoLocalCtxIsJsonObject(value)) {
    config = __rhinoLocalCfgFromJs(value, "", false);
  } else {
    throw __rhinoLocalCtxUnmeasured("config", "(" + __rhinoLocalArgShape([value]) +
      "; only plain objects were measured as accepted)");
  }
  return { returned: "config", config: config, refusal: null };
}

function __rhinoLocalHarvestConfigProvider() {
  __rhinoLocalCtxAudit();
  var assigned = Object.prototype.hasOwnProperty.call(__rhinoLocalTop, "config");
  var result = __rhinoLocalCfgResult(assigned, assigned ? __rhinoLocalTop.config : undefined);
  var effects = JSON.parse(__rhinoLocalHarvest());
  // The context decides no outcome: the wrapped node does.
  effects.outcome = null;
  effects.discardedOutcome = null;
  effects.configProvider = result;
  return JSON.stringify(effects);
}
