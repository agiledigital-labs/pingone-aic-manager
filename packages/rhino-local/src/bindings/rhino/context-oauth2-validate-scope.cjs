// Overlay for AM's OAUTH2_VALIDATE_SCOPE context (legacy 1.0) and
// OAUTH2_VALIDATE_SCOPE_NEXT_GEN (2.0): the OAuth2 provider's scripted scope
// validator. Appended to the shared prelude for a case whose `context` names
// it, after context-common.cjs, whose shared view helpers (__rhinoLocalCtx*)
// it configures with its fault prefix and list kind. Same Rhino 1.7.14
// restrictions as runtime.cjs: no let, no top-level const, no shorthand, no
// Map/Set/Symbol.
//
// Every behaviour here was measured live on 2026-11-10 against a sandbox
// tenant, through client_credentials token requests against a client whose
// provider override named the script, on both engines
// (docs/api/12-script-bindings-matrix.md, "OAuth2 validate scope"). Anything
// the measurement did not reach is a named harness fault
// (`oauth2-validate-scope-unmeasured/<binding>.<member>`), never a guess; see
// docs/rhino-local-gaps.md.
//
// AM runs the script's body, then calls its global validateAccessTokenScope
// with no arguments (a `var f = function` works too) and grants exactly the
// list it returns. The harvest is appended to the author's source and makes
// that call. One case is one access-token request; the refresh and
// authorization-code entry points were not measured.

var __rhinoLocalVs = {
  seed: null,
  // Every exception object the script constructed or the helper threw, so
  // the harvest can tell AM's invalid-scope signal from a lookalike.
  thrown: [],
  // Every legacy set view, so the harvest can read one the script returns.
  sets: [],
};

// Globals the context does not bind (typeof "undefined" on AIC, measured on
// both engines, at the top level and inside the function alike). The
// decision-node names below the first line were not probed here; they are
// unbound in every non-decision context measured so far.
var __rhinoLocalVsUnbound = {
  both: [
    "accessToken", "action", "cacheManager", "callbacks", "claims", "existingSession", "idRepository", "nodeState",
    "outcome", "requestHeaders", "requestParameters", "scopes", "session", "sharedState", "token", "transientState",
    "callbacksBuilder", "requestCookies", "journey", "samlApplication", "oauthApplication", "locales",
    "resumedFromSuspend", "auditEntryDetail", "properties", "config",
  ],
  legacy: [
    "availableScopes", "clientProperties", "cookieName", "emailService", "httpClient", "identity", "jwtAssertion",
    "jwtValidator", "openidm", "policy", "requestProperties", "require", "scopeValidatorHelper", "secrets", "utils",
  ],
  "next-gen": [],
};

// What AM answers, measured on both engines unless noted.
var __rhinoLocalVsGeneric = "Error while running validate scope script";
var __rhinoLocalVsDeclined = "The user has declined authorization";
var __rhinoLocalVsInvalidDefault = "The requested scope is invalid, unknown, or malformed.";
var __rhinoLocalVsDoubleCast = "class java.lang.Double cannot be cast to class java.lang.String (java.lang.Double " +
  "and java.lang.String are in module java.base of loader 'bootstrap')";

// Next-gen scope lists. Measured on all four: length, size(), [i] a JS
// string, contains(), indexOf(), isEmpty(), forEach() of JS strings,
// toArray() a non-array of JS strings, JSON.stringify a JSON array,
// for...in the indices, [].concat(v) the elements, and
// Array.prototype.slice.call(v) a real array. join is Rhino's "Cannot find
// function join", and iterator() the class shutter's refusal.
__rhinoLocalListKinds.oauth2ValidateScope = {
  id: "oauth2-validate-scope-unmeasured/list",
  noun: "a validate scope context list",
  measured: "only length, size(), [i], String(v), contains(), indexOf(), isEmpty(), forEach(), toArray(), " +
    "JSON.stringify, for...in, [].concat(v), Array.prototype.slice.call(v) and, on requestedScopes and " +
    "allowedScopes, add() were measured",
  outOfRange: false,
  concatReceiver: false,
  concatArgument: true,
  json: true,
  genericReceiver: ["slice"],
};

__rhinoLocalCtx.prefix = "oauth2-validate-scope-unmeasured";
__rhinoLocalCtx.noun = "OAuth2 validate scope";
__rhinoLocalCtx.listKind = __rhinoLocalListKinds.oauth2ValidateScope;

// --- Scope lists ---------------------------------------------------------------

function __rhinoLocalVsList(items, label, addable) {
  // The view's size(), get() and String() read this same array, so an add()
  // is seen by every path (measured: String() after add() shows the addition).
  var store = items.slice();
  var list = __rhinoLocalCtxList(store, label);
  __rhinoLocalCtxTag(list, "JavaList", label);
  var str = function (v) {
    if (typeof v !== "string") {
      throw __rhinoLocalCtxUnmeasured(label + ".contains(" + __rhinoLocalArgShape([v]) + ")");
    }
    return v;
  };
  __rhinoLocalCtxMethod(list, label, "java.util.List", "contains", 1, function (v) {
    return store.indexOf(str(v)) !== -1;
  });
  __rhinoLocalCtxMethod(list, label, "java.util.List", "indexOf", 1, function (v) {
    return store.indexOf(str(v));
  });
  __rhinoLocalCtxMethod(list, label, "java.util.List", "isEmpty", 0, function () {
    return store.length === 0;
  });
  __rhinoLocalCtxMethod(list, label, "java.util.List", "forEach", 1, function (fn) {
    if (typeof fn !== "function") {
      throw __rhinoLocalCtxUnmeasured(label + ".forEach(" + __rhinoLocalArgShape(arguments) + ")");
    }
    var copy = store.slice();
    var i;
    for (i = 0; i < copy.length; i += 1) {
      fn(copy[i]);
    }
    return undefined;
  });
  __rhinoLocalCtxMethod(list, label, "java.util.List", "toArray", 0, function () {
    return __rhinoLocalCtxList(store.slice(), label + ".toArray()");
  });
  __rhinoLocalCtxMethod(list, label, "java.util.List", "iterator", 0, function () {
    throw __rhinoLocalShutterError("java.util.ArrayList$Itr");
  });
  __rhinoLocalCtxMethod(list, label, "java.util.List", "add", 1, function (v) {
    if (!addable) {
      throw __rhinoLocalCtxUnmeasured(label + ".add()");
    }
    if (typeof v !== "string") {
      throw __rhinoLocalCtxUnmeasured(label + ".add(" + __rhinoLocalArgShape(arguments) + ")");
    }
    // Measured: true, and the list grows by one even for a value it holds.
    __rhinoLocalCtxCheck(list);
    store.push(v);
    list[store.length - 1] = v;
    __rhinoLocalHide(list, "length", store.length);
    list[__rhinoLocalListKey].items.push(v);
    __rhinoLocalCtxRewatch(list);
    return true;
  });
  return list;
}

// Legacy: requestedScopes is a java.util.HashSet, allowedScopes and
// defaultScopes java.util.TreeSet, all `[object JavaObject]`. Measured on
// them beyond the shared set view: forEach() hands the callback JS strings,
// JSON.stringify writes a JSON array, iterator() of a TreeSet is the class
// shutter's refusal (a HashSet's answers a Java iterator whose members were
// not read), and add() returns true and grows the set.
function __rhinoLocalVsLegacySet(items, label, javaClass) {
  // add() was measured on requestedScopes and allowedScopes only.
  var set = __rhinoLocalCtxJavaSet(items, label, label !== "defaultScopes", javaClass);
  var toArray = set.toArray;
  var strings = function () {
    var arr = toArray.call(set);
    var out = [];
    var i;
    for (i = 0; i < arr.length; i += 1) {
      out.push(String(arr[i]));
    }
    return out;
  };
  __rhinoLocalCtxTag(set, "JavaObject", label);
  __rhinoLocalCtxMethod(set, label, javaClass, "forEach", 1, function (fn) {
    if (typeof fn !== "function") {
      throw __rhinoLocalCtxUnmeasured(label + ".forEach(" + __rhinoLocalArgShape(arguments) + ")");
    }
    var copy = strings();
    var i;
    for (i = 0; i < copy.length; i += 1) {
      fn(copy[i]);
    }
    return undefined;
  });
  __rhinoLocalHide(set, "toJSON", function () {
    return strings();
  });
  // Measured here: toArray() is not a JS array (Array.isArray false), with
  // `length` and items of typeof "object" (Java strings).
  __rhinoLocalCtxMethod(set, label, javaClass, "toArray", 0, function () {
    return __rhinoLocalCtxList(toArray.call(set), label + ".toArray()");
  });
  __rhinoLocalCtxMethod(set, label, javaClass, "iterator", 0, function () {
    if (javaClass === "java.util.TreeSet") {
      throw __rhinoLocalShutterError("java.util.TreeMap$KeyIterator");
    }
    // A HashSet's: `[object JavaObject]`, String(v) Java's class@hash. Its
    // members were not read.
    var it = __rhinoLocalCtxBare();
    __rhinoLocalCtxFaulting(it, label + ".iterator()", ["hasNext", "next", "remove", "forEachRemaining"]);
    __rhinoLocalHide(it, "valueOf", function () {
      return "java.util.HashMap$KeyIterator@rhino-local";
    });
    __rhinoLocalCtxToJSON(it, label + ".iterator()");
    __rhinoLocalCtxTag(it, "JavaObject", label + ".iterator()");
    return it;
  });
  __rhinoLocalVs.sets.push({ view: set, strings: strings });
  return set;
}

// --- Next-gen-only bindings -----------------------------------------------------

// An exception AM reads as "deny with invalid_scope". What a script that
// catches one finds on it was not measured, so its text says so rather than
// imitate AM's. (A faulting toString is not an option: Rhino converts a
// thrown value to a string as it unwinds, whether or not anything reads it.)
function __rhinoLocalVsInvalidScope(message, label) {
  var e = __rhinoLocalJavaException("rhino-local: " + label + " (what a script that catches it reads on AIC " +
    "was not measured)");
  __rhinoLocalVs.thrown.push({ error: e, message: message });
  return e;
}

// Measured: `[object JavaObject]`, String(v) the helper's Java toString
// (class@hash), v.toString() Rhino's "Cannot find function toString", and
// for...in lists throwInvalidScope and inheritAccessTokenScopesOnRefresh.
// throwInvalidScope(msg) denies with 400 invalid_scope and `msg` verbatim;
// (7) gives "7", (null) AM's default text, and a call the script catches is
// swallowed: the function's later return is what AM uses.
function __rhinoLocalVsHelper() {
  var cls = "org.forgerock.oauth2.core.plugins.scripted.ScopeValidatorScriptHelper";
  var helper = __rhinoLocalCtxBare();
  __rhinoLocalCtxMethod(helper, "scopeValidatorHelper", cls, "throwInvalidScope", 1, function (message) {
    var text;
    if (typeof message === "string") {
      text = message;
    } else if (typeof message === "number" && message % 1 === 0) {
      text = String(message);
    } else if (message === null) {
      text = __rhinoLocalVsInvalidDefault;
    } else {
      throw __rhinoLocalCtxUnmeasured("scopeValidatorHelper.throwInvalidScope(" + __rhinoLocalArgShape(arguments) + ")");
    }
    throw __rhinoLocalVsInvalidScope(text, "scopeValidatorHelper.throwInvalidScope()");
  });
  // Measured: with no arguments it answers undefined and throws nothing on a
  // client_credentials request. A refresh grant, where it would matter, was
  // not measured; one case is one access-token request.
  __rhinoLocalCtxMethod(helper, "scopeValidatorHelper", cls, "inheritAccessTokenScopesOnRefresh", 0, function () {
    return undefined;
  });
  __rhinoLocalCtxFaulting(helper, "scopeValidatorHelper", [
    "getClass", "hashCode", "equals", "wait", "notify", "notifyAll",
  ]);
  __rhinoLocalHide(helper, "valueOf", function () {
    return cls + "@rhino-local";
  });
  __rhinoLocalCtxToJSON(helper, "scopeValidatorHelper");
  __rhinoLocalCtxTag(helper, "JavaObject", "scopeValidatorHelper");
  return __rhinoLocalCtxWatch(helper, "scopeValidatorHelper");
}

// Measured on client_credentials, where no user is involved: `[object
// JavaObject]`, exists() false, getName() AM's NullPointerException as an
// InternalError. A request that carries a user was not measured.
function __rhinoLocalVsIdentity() {
  var cls = "org.forgerock.openam.scripting.api.identity.ScriptedIdentityScriptWrapper";
  var identity = __rhinoLocalCtxBare();
  __rhinoLocalCtxMethod(identity, "identity", cls, "exists", 0, function () {
    return false;
  });
  __rhinoLocalCtxMethod(identity, "identity", cls, "getName", 0, function () {
    throw __rhinoLocalInternalError(
      'Cannot invoke "com.sun.identity.idm.AMIdentity.getName()" because "this.amIdentity" is null');
  });
  __rhinoLocalCtxFaulting(identity, "identity", [
    "getUniversalId", "getAttributeValues", "setAttribute", "addAttribute", "store", "getClass", "hashCode", "equals",
    "wait", "notify", "notifyAll",
  ]);
  __rhinoLocalHide(identity, "valueOf", function () {
    return cls + "@rhino-local";
  });
  __rhinoLocalCtxToJSON(identity, "identity");
  __rhinoLocalCtxTag(identity, "JavaObject", "identity");
  return __rhinoLocalCtxWatch(identity, "identity");
}

// --- Legacy-only: AM's exception class ------------------------------------------

// Measured on legacy: `throw new org.forgerock.oauth2.core.exceptions.
// InvalidScopeException(msg)` denies with 400 invalid_scope and `msg`. On
// next-gen the same throw is a 500, and the class is left to the shutter.
function __rhinoLocalVsLegacyException(message) {
  if (arguments.length !== 1 || typeof message !== "string") {
    throw __rhinoLocalCtxUnmeasured("new InvalidScopeException(" + __rhinoLocalArgShape(arguments) + ")");
  }
  return __rhinoLocalVsInvalidScope(message, "InvalidScopeException");
}

// --- Seed ----------------------------------------------------------------------

function __rhinoLocalVsDefaults(given) {
  var tokenRealm = given.realm !== undefined ? given.realm : "/alpha";
  var bareRealm = String(tokenRealm).replace(/^\//, "") || "root";
  var v = given.validateScope || {};
  var requested = v.requested !== undefined ? v.requested.slice() : [];
  var allowed = v.allowed !== undefined ? v.allowed.slice() : [];
  var c = given.client || {};
  var clientId = c.clientId !== undefined ? c.clientId : "test-client";
  var r = given.request || {};
  var params = { client_id: [clientId], grant_type: ["client_credentials"] };
  if (requested.length > 0) {
    params.scope = [requested.join(" ")];
  }
  return {
    realm: tokenRealm,
    requested: requested,
    allowed: allowed,
    available: v.available !== undefined ? v.available.slice() : allowed.slice(),
    defaults: v.defaults !== undefined ? v.defaults.slice() : [],
    client: {
      clientId: clientId,
      allowedGrantTypes: c.allowedGrantTypes !== undefined ? c.allowedGrantTypes.slice() : ["CLIENT_CREDENTIALS"],
      allowedScopes: c.allowedScopes !== undefined ? c.allowedScopes.slice() : allowed.slice(),
      allowedResponseTypes: c.allowedResponseTypes !== undefined ? c.allowedResponseTypes.slice() : [],
      customProperties: c.customProperties !== undefined ? __rhinoLocalCtxCopy(c.customProperties) : {},
      redirectUris: c.redirectUris !== undefined ? c.redirectUris.slice() : [],
    },
    request: {
      // AM filters client_secret out before the script sees it (measured:
      // client_id, grant_type and scope are the parameters listed).
      params: r.params !== undefined ? __rhinoLocalCtxCopy(r.params) : params,
      headers: r.headers !== undefined ? __rhinoLocalCtxCopy(r.headers) : {},
      uri: r.uri !== undefined
        ? r.uri
        : "https://tenant.example.com/am/oauth2/realms/root/realms/" + bareRealm + "/access_token",
    },
  };
}

function __rhinoLocalSeedValidateScope(given) {
  given = given || {};
  __rhinoLocalCtxBegin(given.engine);
  var engine = __rhinoLocalCtx.engine;
  var legacy = engine === "legacy";
  // logger.getName() is a java.lang.String on legacy (measured), and the
  // sets' toArray() items are Java strings (typeof "object", measured).
  __rhinoLocalCtx.realJavaStrings = legacy;
  __rhinoLocalCtxUnbind(__rhinoLocalVsUnbound.both.concat(__rhinoLocalVsUnbound[engine]));
  var d = __rhinoLocalVsDefaults(given);
  __rhinoLocalVs.seed = d;
  __rhinoLocalVs.thrown = [];
  __rhinoLocalVs.sets = [];
  if (given.realm === undefined) {
    realm = d.realm;
  }
  var loggerName = "scripts.OAUTH2_VALIDATE_SCOPE" + (legacy ? "" : "_NEXT_GEN") + "." +
    __rhinoLocal.loggerScriptId + ".(" + scriptName + ")";
  __rhinoLocalHide(logger, "getName", function () {
    return legacy ? __rhinoLocalCtxJavaString(loggerName) : loggerName;
  });
  if (legacy) {
    requestedScopes = __rhinoLocalVsLegacySet(d.requested, "requestedScopes", "java.util.HashSet");
    allowedScopes = __rhinoLocalVsLegacySet(d.allowed, "allowedScopes", "java.util.TreeSet");
    defaultScopes = __rhinoLocalVsLegacySet(d.defaults, "defaultScopes", "java.util.TreeSet");
    org.forgerock.oauth2 = {
      core: { exceptions: { InvalidScopeException: __rhinoLocalVsLegacyException } },
    };
  } else {
    requestedScopes = __rhinoLocalVsList(d.requested, "requestedScopes", true);
    allowedScopes = __rhinoLocalVsList(d.allowed, "allowedScopes", true);
    availableScopes = __rhinoLocalVsList(d.available, "availableScopes", false);
    defaultScopes = __rhinoLocalVsList(d.defaults, "defaultScopes", false);
    scopeValidatorHelper = __rhinoLocalVsHelper();
    identity = __rhinoLocalVsIdentity();
    requestProperties = __rhinoLocalCtxBuildRequest(d.request, d.realm, { paramsGetMissing: true, javaTags: true });
    __rhinoLocalCtxTag(requestProperties, "JavaMap", "requestProperties");
    clientProperties = __rhinoLocalCtxBuildClient(d.client, { redirectUris: true, javaTags: true });
    __rhinoLocalCtxTag(clientProperties, "JavaMap", "clientProperties");
  }
  __rhinoLocalCtxSeal();
}

// --- Harvest -------------------------------------------------------------------

function __rhinoLocalVsThrown(e) {
  var i;
  for (i = 0; i < __rhinoLocalVs.thrown.length; i += 1) {
    if (__rhinoLocalVs.thrown[i].error === e) {
      return __rhinoLocalVs.thrown[i];
    }
  }
  return null;
}

function __rhinoLocalVsIsJavaString(value) {
  return typeof java !== "undefined" && value !== null && typeof value === "object" &&
    value instanceof java.lang.String;
}

// The scopes AM grants for a returned value, or the refusal it answers.
function __rhinoLocalVsResult(value) {
  var none = function (status, refusal) {
    return { returned: "none", status: status, granted: null, invalidScope: null, refusal: refusal };
  };
  var items = null;
  var row;
  var i;
  if (value === null || value === undefined) {
    return none(403, __rhinoLocalVsDeclined);
  }
  if (typeof value === "string") {
    return none(500, __rhinoLocalVsGeneric);
  }
  row = __rhinoLocalListRow(value);
  if (row !== null) {
    items = row.items.slice();
  }
  for (i = 0; items === null && i < __rhinoLocalVs.sets.length; i += 1) {
    if (__rhinoLocalVs.sets[i].view === value) {
      items = __rhinoLocalVs.sets[i].strings();
    }
  }
  if (items === null && Array.isArray(value)) {
    items = value.slice();
  }
  if (items === null) {
    if (typeof value === "object" && Object.prototype.toString.call(value) === "[object Object]") {
      return none(500, __rhinoLocalVsGeneric);
    }
    throw __rhinoLocalCtxUnmeasured("validateAccessTokenScope()", "returning " + __rhinoLocalArgShape([value]));
  }
  var granted = [];
  var item;
  for (i = 0; i < items.length; i += 1) {
    item = items[i];
    if (typeof item === "number") {
      return none(500, __rhinoLocalVsDoubleCast);
    }
    if (__rhinoLocalVsIsJavaString(item)) {
      item = String(item);
    }
    if (typeof item !== "string") {
      throw __rhinoLocalCtxUnmeasured("validateAccessTokenScope()[" + i + "]", "returning " + __rhinoLocalArgShape([item]));
    }
    if (granted.indexOf(item) === -1) {
      granted.push(item);
    }
  }
  if (granted.length === 0) {
    return none(403, __rhinoLocalVsDeclined);
  }
  return { returned: "scopes", status: 200, granted: granted, invalidScope: null, refusal: null };
}

function __rhinoLocalHarvestValidateScope() {
  var fn = Object.prototype.hasOwnProperty.call(__rhinoLocalTop, "validateAccessTokenScope")
    ? __rhinoLocalTop.validateAccessTokenScope
    : undefined;
  var result;
  if (fn === undefined) {
    result = { returned: "none", status: 500, granted: null, invalidScope: null, refusal: __rhinoLocalVsGeneric };
  } else if (typeof fn !== "function") {
    throw __rhinoLocalCtxUnmeasured("validateAccessTokenScope", "as a " + typeof fn + " rather than a function");
  } else {
    var value;
    var signalled = null;
    try {
      value = fn.call(__rhinoLocalTop);
    } catch (e) {
      signalled = __rhinoLocalVsThrown(e);
      if (signalled === null) {
        throw e;
      }
    }
    result = signalled !== null
      ? { returned: "none", status: 400, granted: null, invalidScope: signalled.message, refusal: signalled.message }
      : __rhinoLocalVsResult(value);
  }
  __rhinoLocalCtxAudit();
  var effects = JSON.parse(__rhinoLocalHarvest());
  // The context decides no outcome: AM's token endpoint does.
  effects.outcome = null;
  effects.discardedOutcome = null;
  effects.validateScope = result;
  return JSON.stringify(effects);
}

var requestedScopes;
var availableScopes;
var allowedScopes;
var defaultScopes;
var scopeValidatorHelper;
var identity;
var requestProperties;
var clientProperties;
