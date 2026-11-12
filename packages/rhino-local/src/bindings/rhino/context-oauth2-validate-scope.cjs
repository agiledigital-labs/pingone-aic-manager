// Overlay for AM's OAUTH2_VALIDATE_SCOPE_NEXT_GEN context (2.0): the OAuth2
// provider's scripted scope validator. Appended to the shared prelude for a case whose `context` names
// it, after context-common.cjs, whose shared view helpers (__rhinoLocalCtx*)
// it configures with its fault prefix and list kind. Same Rhino 1.7.14
// restrictions as runtime.cjs: no let, no top-level const, no shorthand, no
// Map/Set/Symbol.
//
// Every behaviour here was measured live on 2026-11-10 against a sandbox
// tenant, through client_credentials token requests against a client whose
// provider override named the script
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
};

// Globals the context does not bind (typeof "undefined" on AIC, measured at
// the top level and inside the function alike). The names from
// `callbacksBuilder` on were not probed here; they are unbound in every
// non-decision context measured so far.
var __rhinoLocalVsUnbound = [
  "accessToken", "action", "cacheManager", "callbacks", "claims", "existingSession", "idRepository", "nodeState",
  "outcome", "requestHeaders", "requestParameters", "scopes", "session", "sharedState", "token", "transientState",
  "callbacksBuilder", "requestCookies", "journey", "samlApplication", "oauthApplication", "locales",
  "resumedFromSuspend", "auditEntryDetail", "properties", "config",
];

// What AM answers.
var __rhinoLocalVsGeneric = "Error while running validate scope script";
var __rhinoLocalVsDeclined = "The user has declined authorization";
var __rhinoLocalVsInvalidDefault = "The requested scope is invalid, unknown, or malformed.";
var __rhinoLocalVsDoubleCast = "class java.lang.Double cannot be cast to class java.lang.String (java.lang.Double " +
  "and java.lang.String are in module java.base of loader 'bootstrap')";

// Scope lists are real Java lists (runtime.cjs, __rhinoLocalListView).
// Measured on all four: `[object JavaList]`, length, size(), [i] a JS
// string, contains(), indexOf(), isEmpty(), forEach() of JS strings,
// toArray() a non-array, JSON.stringify a JSON array, for...in the indices,
// [].concat(v) the elements, Array.prototype.slice.call(v) a real array,
// add() returning true and growing the list; join is "Cannot find function
// join", and iterator() the class shutter's refusal.

__rhinoLocalCtx.prefix = "oauth2-validate-scope-unmeasured";
__rhinoLocalCtx.noun = "OAuth2 validate scope";

// --- Scope lists ---------------------------------------------------------------

function __rhinoLocalVsList(items) {
  return __rhinoLocalListView(items.slice());
}

// An exception AM reads as "deny with invalid_scope". The harvest matches the
// thrown object by identity and uses the argument the script passed, which is
// the measured 400 text. What a script reads or renders was not measured, so
// message, name, String(e) and JSON.stringify(e) fault even inside catch (Sol
// ctx r5 P2-4, r6 P2-1).
//
// This is a plain object, not an Error. Rhino's JavaScriptException reads
// `.name` once while it wraps a NativeError, before the script's catch runs,
// and a getter on Error's `message` slot never runs. Either one turns the
// measured uncaught 400, and the measured catch-and-return, into a fault.
// Wrapping a plain object calls toString once, from JavaScriptException
// .getDetails, to fill the Java message. That frame is the engine's, so the
// call returns a constant and does not fault. Any other toString is the
// script's.
function __rhinoLocalVsRead(label, what) {
  throw __rhinoLocalCtxUnmeasured(label + "." + what,
    "of the caught invalid-scope exception (what a script reads on it was not measured)");
}

function __rhinoLocalVsRhinoWrap() {
  if (__rhinoLocalCtxHostOp === null) {
    return false;
  }
  var flag = __rhinoLocalCtxHostOp("exceptionDetails");
  return flag === true || String(flag) === "true";
}

function __rhinoLocalVsInvalidScope(message, label) {
  var e = {};
  __rhinoLocalVs.thrown.push({ error: e, message: message });
  Object.defineProperty(e, "message", {
    configurable: true,
    enumerable: false,
    get: function () { __rhinoLocalVsRead(label, "message"); },
  });
  Object.defineProperty(e, "name", {
    configurable: true,
    enumerable: false,
    get: function () { __rhinoLocalVsRead(label, "name"); },
  });
  __rhinoLocalHide(e, "toString", function () {
    if (__rhinoLocalVsRhinoWrap()) {
      return "JavaException";
    }
    __rhinoLocalVsRead(label, "toString");
  });
  __rhinoLocalCtxToJSON(e, label);
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
  __rhinoLocalCtxBegin();
  __rhinoLocalCtxUnbind(__rhinoLocalVsUnbound);
  var d = __rhinoLocalVsDefaults(given);
  __rhinoLocalVs.seed = d;
  __rhinoLocalVs.thrown = [];
  if (given.realm === undefined) {
    realm = d.realm;
  }
  var loggerName = "scripts.OAUTH2_VALIDATE_SCOPE_NEXT_GEN." +
    __rhinoLocal.loggerScriptId + ".(" + scriptName + ")";
  __rhinoLocalHide(logger, "getName", function () {
    return loggerName;
  });
  requestedScopes = __rhinoLocalVsList(d.requested);
  allowedScopes = __rhinoLocalVsList(d.allowed);
  availableScopes = __rhinoLocalVsList(d.available);
  defaultScopes = __rhinoLocalVsList(d.defaults);
  scopeValidatorHelper = __rhinoLocalVsHelper();
  identity = __rhinoLocalVsIdentity();
  requestProperties = __rhinoLocalCtxBuildRequest(d.request, d.realm, {
    paramsGetMissing: true, javaTags: true,
  });
  __rhinoLocalCtxTag(requestProperties, "JavaMap", "requestProperties");
  clientProperties = __rhinoLocalCtxBuildClient(d.client, {
    redirectUris: true,
    javaTags: true,
  });
  __rhinoLocalCtxTag(clientProperties, "JavaMap", "clientProperties");
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

// The scopes AM grants for a returned value, or the refusal it answers.
function __rhinoLocalVsResult(value) {
  var none = function (status, refusal) {
    return { returned: "none", status: status, granted: null, invalidScope: null, refusal: refusal };
  };
  var items = null;
  var i;
  if (value === null || value === undefined) {
    return none(403, __rhinoLocalVsDeclined);
  }
  if (typeof value === "string") {
    return none(500, __rhinoLocalVsGeneric);
  }
  items = __rhinoLocalListItems(value);
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
