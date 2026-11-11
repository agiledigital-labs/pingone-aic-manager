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
// library-unmeasured/<binding>.<member>. String() and JSON.stringify of those
// four were not measured either (library-unmeasured/String(<binding>) and
// library-unmeasured/JSON.stringify(<binding>)). logger.getName was called
// with no arguments; any other count is library-unmeasured/logger.getName(…).
// The shared objects the probe called (that zero-argument getName,
// systemEnv.getProperty of a missing key, utils.base64.encode, openidm.read
// of a missing id) keep the harness implementations. A returned value is
// admitted only when it is JSON (nested null included) or top-level
// undefined. JSON.stringify is not that check.

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
  // The probe recorded typeof "object" and did not render the object.
  // String() and JSON.stringify would otherwise publish a synthetic
  // "[object Object]" and "{}".
  __rhinoLocalHide(target, "toString", function () {
    throw __rhinoLocalCtxUnmeasured("String(" + label + ")");
  });
  __rhinoLocalCtxToJSON(target, label);
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
  // Only the zero-argument call was measured. The class name is unused: a
  // wrong count is an unmeasured fault, not a guessed overload error.
  __rhinoLocalCtxMethod(logger, "logger", "logger", "getName", 0, function () {
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

function __rhinoLocalLibRefuse() {
  throw __rhinoLocalFault(
    "library-unmeasured/return",
    "rhino-local: the library export returned a value JSON cannot carry"
  );
}

function __rhinoLocalLibIsJava(value) {
  try {
    return value instanceof java.lang.Object;
  } catch (error) {
    void error;
    return false;
  }
}

// A copy of a value JSON carries exactly. Nested null stays null. A nested
// undefined, a function, a non-finite number (including -0), a Date, an
// Error, a cycle or a Java object is not a captured return: stringify would
// drop or rewrite it, and that rewrite was not measured.
function __rhinoLocalLibAdmit(value, path) {
  var i;
  var keys;
  var out;
  if (value === undefined) {
    __rhinoLocalLibRefuse();
  }
  if (value === null || typeof value === "string" || typeof value === "boolean") {
    return value;
  }
  if (typeof value === "number") {
    if (!isFinite(value) || (value === 0 && 1 / value < 0)) {
      __rhinoLocalLibRefuse();
    }
    return value;
  }
  if (typeof value !== "object" || __rhinoLocalLibIsJava(value) || path.indexOf(value) !== -1 ||
      !__rhinoLocalCtxPlainJson(value)) {
    __rhinoLocalLibRefuse();
  }
  path.push(value);
  if (Array.isArray(value)) {
    out = [];
    for (i = 0; i < value.length; i += 1) {
      out.push(__rhinoLocalLibAdmit(value[i], path));
    }
  } else {
    out = {};
    keys = Object.keys(value);
    for (i = 0; i < keys.length; i += 1) {
      Object.defineProperty(out, keys[i], {
        value: __rhinoLocalLibAdmit(value[keys[i]], path),
        enumerable: true,
        configurable: true,
        writable: true
      });
    }
  }
  path.pop();
  return out;
}

function __rhinoLocalLibValue(value) {
  if (value === undefined) {
    return { undefined: true, value: null };
  }
  return { undefined: false, value: __rhinoLocalLibAdmit(value, []) };
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
