// Overlay for AM's CONFIG_PROVIDER_NODE context (next-gen): the
// Configuration Provider node's script, which builds the configuration of the
// node it wraps at journey runtime. Appended to the shared prelude for a case
// whose `context` names it, after context-common.cjs. Same Rhino 1.7.14
// restrictions as runtime.cjs: no let, no top-level const, no shorthand, no
// Map/Set/Symbol.
//
// Every behaviour here was measured live on 2026-11-10 against a sandbox
// tenant, by running a journey (a decision node seeding the node state, a
// ConfigProviderNode wrapping a MessageNode, a decision node reading back)
// (docs/api/12-script-bindings-matrix.md, "Config provider
// node"). Anything the measurement did not reach is a named harness fault
// (`config-provider-unmeasured/<binding>.<member>`), never a guess; see
// docs/rhino-local-gaps.md.
//
// AM reads the script's `config` global after the script ends; a script that
// only evaluates to a map is refused. The harvest is appended to the author's
// source, like the access-token context's. The node runs the script again on
// every request that reaches it (callback submission included); one case is
// one invocation.

// Globals the context does not bind (typeof "undefined" on AIC, measured).
// existingSession is in the next-gen descriptor but is a ReferenceError on a
// request without a session cookie (measured; with one, unmeasured). `config` is unbound before the script (measured: a
// ReferenceError), whatever a previous case left.
var __rhinoLocalCfgUnbound = [
  "config", "sharedState", "transientState", "outcome", "existingSession", "identity", "session",
  "auditEntryDetail", "properties", "cacheManager",
];

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
__rhinoLocalCtx.catalog = "config";
__rhinoLocalCtx.noun = "config provider node";
__rhinoLocalCtx.listKind = __rhinoLocalListKinds.configProvider;

// --- Request maps -------------------------------------------------------------

// Measured: requestHeaders.get(name) and
// requestParameters.get(name) are `[object JavaList]` whose String(v) is
// `[v1]` and whose length is the value count; get of an absent name is null;
// for...in counts the entries. Every path to an entry (get, dot access,
// for...in) reads one store of those tagged lists (Sol ctx r4 P2-1), so the
// context's list policy applies whichever way a script reaches a value.
// forEach, String(map) and the rest of java.util.Map were not measured here
// and fault. runtime.cjs's decision-node maps are replaced whole.
function __rhinoLocalCfgRequestMap(values, caseInsensitive, label) {
  var lists = __rhinoLocalCtxBare();
  var keys = [];
  Object.keys(values || {}).forEach(function (raw) {
    var key = caseInsensitive ? raw.toLowerCase() : raw;
    var items = typeof values[raw] === "string" ? [values[raw]] : values[raw].slice();
    var itemLabel = label + ".get(" + JSON.stringify(key) + ")";
    var list = __rhinoLocalCtxList(items.slice(), itemLabel, null, label);
    __rhinoLocalCtxTag(list, "JavaList", itemLabel);
    __rhinoLocalCtxRewatch(list);
    if (keys.indexOf(key) === -1) {
      keys.push(key);
    }
    lists[key] = list;
  });
  return __rhinoLocalCtxMap(label, keys, function (k) {
    return lists[k];
  }, null, caseInsensitive ? __rhinoLocalCtxHeaderAliases(keys) : null, {
    get: {
      arity: 1,
      fn: function (name) {
        var key = caseInsensitive ? String(name).toLowerCase() : String(name);
        return Object.prototype.hasOwnProperty.call(lists, key) ? lists[key] : null;
      },
    },
  });
}

// String(v) of the next-gen views below was not measured; it faults rather
// than letting Rhino's own TypeError read as AIC's answer (Sol ctx r4 P2-4).
function __rhinoLocalCfgNoRender(out, label) {
  __rhinoLocalHide(out, "toString", function () {
    throw __rhinoLocalCtxUnmeasured("String(" + label + ")");
  });
  __rhinoLocalHide(out, "valueOf", function () {
    throw __rhinoLocalCtxUnmeasured("String(" + label + ")");
  });
}

// --- The bindings whose behaviour belongs to the wrapped node ------------------

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
  __rhinoLocalCfgNoRender(out, "callbacks");
  __rhinoLocalCtxTag(out, null, "callbacks");
  return __rhinoLocalCtxWatch(out, "callbacks");
}

// Measured: callbacksBuilder.textOutputCallback(0, "...") throws nothing and
// adds no callback to what the wrapped node shows. Every other method is
// unmeasured here.
function __rhinoLocalCfgCallbacksBuilder() {
  var out = __rhinoLocalCtxBare();
  // Measured with (0, string) only; the descriptor declares a numeric
  // message type and a string message. Other shapes fault (Sol ctx r4 P2-3).
  __rhinoLocalCtxMethod(out, "callbacksBuilder", "CallbacksBuilder", "textOutputCallback", 2, function (type, message) {
    if (typeof type !== "number" || type % 1 !== 0 || typeof message !== "string") {
      throw __rhinoLocalCtxUnmeasured("callbacksBuilder.textOutputCallback(" + __rhinoLocalArgShape(arguments) + ")");
    }
    return undefined;
  });
  __rhinoLocalCtxFaulting(out, "callbacksBuilder", __rhinoLocalCfgBuilderMethods);
  __rhinoLocalCtxToJSON(out, "callbacksBuilder");
  __rhinoLocalCfgNoRender(out, "callbacksBuilder");
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
  __rhinoLocalCfgNoRender(out, "action");
  __rhinoLocalCtxTag(out, null, "action");
  return __rhinoLocalCtxWatch(out, "action");
}

// --- Seed ----------------------------------------------------------------------

// Runs after __rhinoLocalSeed, on the same seed object.
function __rhinoLocalSeedConfigProvider(given) {
  given = given || {};
  __rhinoLocalCtxBegin();
  __rhinoLocalCtxUnbind(__rhinoLocalCfgUnbound);
  if (given.realm === undefined) {
    realm = "/alpha";
  }
  var loggerName = "scripts.CONFIG_PROVIDER_NODE_NEXT_GEN." +
    __rhinoLocal.loggerScriptId + ".(" + scriptName + ")";
  __rhinoLocalHide(logger, "getName", function () {
    return loggerName;
  });
  requestHeaders = __rhinoLocalCfgRequestMap(given.requestHeaders, true, "requestHeaders");
  requestParameters = __rhinoLocalCfgRequestMap(given.requestParameters, false, "requestParameters");
  // Measured: the context's class shutter refuses the Java set keys() builds.
  __rhinoLocalHide(nodeState, "keys", function () {
    throw __rhinoLocalShutterError("java.util.HashSet");
  });
  callbacks = __rhinoLocalCfgCallbacks();
  callbacksBuilder = __rhinoLocalCfgCallbacksBuilder();
  action = __rhinoLocalCfgAction();
  __rhinoLocalCtxSeal();
}

// --- Harvest ---------------------------------------------------------------------

function __rhinoLocalCfgIsJava(value) {
  return typeof java !== "undefined" && value !== null && typeof value === "object" &&
    value instanceof java.lang.Object;
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
  if (__rhinoLocalCfgIsJava(value) || !__rhinoLocalCtxIsJsonObject(value)) {
    throw __rhinoLocalCtxUnmeasured("config", "(" + __rhinoLocalArgShape([value]) +
      "; only plain objects were measured as accepted)");
  }
  var config = __rhinoLocalCfgFromJs(value, "", false);
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
