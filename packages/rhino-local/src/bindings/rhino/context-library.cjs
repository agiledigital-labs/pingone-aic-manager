// Overlay for a direct LIBRARY run. Appended to the shared prelude after
// context-common.cjs when case.context is "LIBRARY". Same Rhino 1.7.14
// restrictions as runtime.cjs: no let, no top-level const, no shorthand,
// no Map/Set/Symbol.
//
// A library is not an entry point. AM runs it when some next-gen script
// require()s it, inside (function (require, exports, module) { ... }). The
// caller's context bindings do not leak (measured 2026-11-11: nodeState from
// a decision node and accessToken from token modification are both
// ReferenceErrors inside the library). scriptName and logger.getName() are
// the caller's, including inside a nested require(). Legacy engines have no
// require(); this context is next-gen only.
//
// The case's script is the library body. The runner require()s it through
// the existing loader and calls the named export. Anything this measurement
// did not reach on emailService, jwtAssertion, jwtValidator or policy is
// library-unmeasured/<binding>.<member>. The shared objects the probe called
// (logger.getName, systemEnv.getProperty of a missing key, utils.base64.encode,
// openidm.read of a missing id) keep the harness implementations.

var __rhinoLocalLibUnbound = [
  "accessToken", "action", "allowedScopes", "auditEntryDetail", "availableScopes",
  "cacheManager", "callbacks", "callbacksBuilder", "claims", "claimsLocales",
  "clientProperties", "config", "console", "defaultScopes", "existingSession",
  "idRepository", "identity", "journey", "locales", "nodeState", "oauthApplication",
  "outcome", "process", "properties", "rawProfile", "requestCookies", "requestHeaders",
  "requestParameters", "requestProperties", "requestedClaims", "requestedScopes",
  "resumedFromSuspend", "samlApplication", "scopeValidatorHelper", "scopes",
  "selectedIdp", "session", "sharedState", "token", "transientState"
];

function __rhinoLocalLibFaultObject(target, label) {
  var names = [];
  var key;
  for (key in target) {
    if (Object.prototype.hasOwnProperty.call(target, key) && typeof target[key] === "function") {
      names.push(key);
    }
  }
  var i;
  for (i = 0; i < names.length; i += 1) {
    (function (name) {
      __rhinoLocalHide(target, name, function () {
        throw __rhinoLocalFault(
          "library-unmeasured/" + label + "." + name,
          "rhino-local: " + label + "." + name +
            " in the library context is unmeasured; docs/api/12-script-bindings-matrix.md lists what was measured"
        );
      });
    })(names[i]);
  }
}

function __rhinoLocalSeedLibrary(given) {
  given = given || {};
  __rhinoLocalCtxBegin(given.engine || "next-gen");
  __rhinoLocalCtx.prefix = "library-unmeasured";
  __rhinoLocalCtx.noun = "library";
  // A missing ESV reads back as null (measured). The decision-node default
  // of throwing is a fixture guard, not what a library sees.
  if (given.esvUndeclared === undefined) {
    __rhinoLocal.esvUndeclared = "absent";
  }
  __rhinoLocalCtxUnbind(__rhinoLocalLibUnbound);
  var spec = given.library || {};
  var caller = spec.callerContext || "AUTHENTICATION_TREE_DECISION_NODE";
  __rhinoLocal.libraryCall = {
    id: spec.id ? String(spec.id) : "",
    exportName: spec["export"] ? String(spec["export"]) : "",
    args: spec.args || [],
    callerContext: String(caller)
  };
  var loggerName = "scripts." + __rhinoLocal.libraryCall.callerContext + "." +
    __rhinoLocal.loggerScriptId + ".(" + scriptName + ")";
  __rhinoLocalHide(logger, "getName", function () {
    return loggerName;
  });
  __rhinoLocalLibFaultObject(emailService, "emailService");
  __rhinoLocalLibFaultObject(jwtAssertion, "jwtAssertion");
  __rhinoLocalLibFaultObject(jwtValidator, "jwtValidator");
  __rhinoLocalLibFaultObject(policy, "policy");
}

function __rhinoLocalLibThrown(error) {
  var named = error && typeof error === "object" && error.name ? String(error.name) : "";
  return { type: typeof error, name: named, message: String(error) };
}

function __rhinoLocalLibValue(value) {
  if (value === undefined) {
    return { undefined: true, value: null };
  }
  var encoded;
  try {
    encoded = JSON.parse(JSON.stringify(value));
  } catch (error) {
    encoded = undefined;
    void error;
  }
  if (encoded === undefined) {
    throw __rhinoLocalFault(
      "library-unmeasured/return",
      "rhino-local: the library export returned a value JSON cannot carry"
    );
  }
  return { undefined: false, value: encoded };
}

function __rhinoLocalLibFinish(result) {
  var effects = JSON.parse(__rhinoLocalHarvest());
  effects.outcome = null;
  effects.discardedOutcome = null;
  effects.returns = result;
  return JSON.stringify(effects);
}

function __rhinoLocalHarvestLibrary() {
  var call = __rhinoLocal.libraryCall;
  var result = {
    status: "returned",
    phase: "export",
    type: "",
    name: "",
    message: "",
    undefined: false,
    value: null
  };
  var lib;
  var thrown;
  try {
    lib = require(call.id);
  } catch (error) {
    thrown = __rhinoLocalLibThrown(error);
    result.status = "thrown";
    result.phase = "module";
    result.type = thrown.type;
    result.name = thrown.name;
    result.message = thrown.message;
    return __rhinoLocalLibFinish(result);
  }
  try {
    var value = lib[call.exportName].apply(lib, call.args);
    var carried = __rhinoLocalLibValue(value);
    result.undefined = carried.undefined;
    result.value = carried.value;
  } catch (error) {
    thrown = __rhinoLocalLibThrown(error);
    result.status = "thrown";
    result.phase = "export";
    result.type = thrown.type;
    result.name = thrown.name;
    result.message = thrown.message;
  }
  return __rhinoLocalLibFinish(result);
}
