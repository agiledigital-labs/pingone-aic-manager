// zzemuC7 library probe. Records typeof only, plus a few scalars we control.
// Never serialises request, requestHeaders, existingSession, context or identity.

var NAMES = [
  "action", "auditEntryDetail", "availableScopes", "allowedScopes", "accessToken",
  "callbacks", "callbacksBuilder", "cacheManager", "claims", "clientProperties",
  "config", "console", "context", "cookieName", "defaultScopes", "emailService",
  "existingSession", "exports", "httpClient", "identity", "idRepository",
  "JavaImporter", "journey", "jwtAssertion", "jwtValidator", "locales", "logger",
  "module", "nodeState", "oauthApplication", "openidm", "outcome", "policy",
  "process", "properties", "rawProfile", "realm", "requestCookies", "requestHeaders",
  "requestParameters", "requestProperties", "requestedScopes", "require",
  "resumedFromSuspend", "samlApplication", "scopeValidatorHelper", "scopes",
  "scriptName", "secrets", "selectedIdp", "session", "sharedState", "systemEnv",
  "token", "transientState", "utils"
];

function typeOfName(name) {
  try {
    return typeof eval(name);
  } catch (e) {
    return "throw";
  }
}

function safe(fn) {
  try {
    return fn();
  } catch (e) {
    return "err";
  }
}

function snap() {
  var out = {};
  var i;
  for (i = 0; i < NAMES.length; i++) {
    out[NAMES[i]] = typeOfName(NAMES[i]);
  }
  out.scriptNameV = safe(function () { return String(scriptName); });
  out.realmV = safe(function () { return String(realm); });
  out.cookieKind = safe(function () {
    if (typeof cookieName !== "string") return typeof cookieName;
    if (cookieName === "iPlanetDirectoryPro") return "iPlanetDirectoryPro";
    return "other-len-" + String(cookieName.length);
  });
  out.loggerName = safe(function () { return String(logger.getName()); });
  out.envMiss = safe(function () {
    var v = systemEnv.getProperty("esv.zzemuC7.missing");
    return v === null ? "null" : typeof v;
  });
  out.b64 = safe(function () { return String(utils.base64.encode("zz")); });
  out.oidMiss = safe(function () {
    var rec = openidm.read("managed/alpha_user/zzemuC7-missing");
    return rec === null ? "null" : typeof rec;
  });
  out.callerMark = safe(function () {
    return typeof zzemuC7CallerMark === "undefined" ? "absent" : String(zzemuC7CallerMark);
  });
  out.innerMark = safe(function () {
    return typeof zzemuC7InnerMark === "undefined" ? "absent" : String(zzemuC7InnerMark);
  });
  out.hashSet = safe(function () {
    var s = new java.util.HashSet();
    s.add("a");
    return "ok-" + String(s.size());
  });
  return out;
}

var moduleSnap = snap();

exports.surface = function () {
  var nested = "skip";
  var nestedName = "skip";
  var nestedLogger = "skip";
  var nestedNode = "skip";
  try {
    var inner = require("zzemuC7-inner");
    var stamp = inner.stamp();
    nested = stamp.tag;
    nestedName = stamp.scriptName;
    nestedLogger = stamp.loggerName;
    nestedNode = stamp.nodeState;
  } catch (e) {
    nested = "err";
  }
  return {
    module: moduleSnap,
    fn: snap(),
    nested: nested,
    nestedName: nestedName,
    nestedLogger: nestedLogger,
    nestedNode: nestedNode,
    thisType: typeof this,
    thisIsExports: this === exports,
    thisIsModuleExports: typeof module !== "undefined" && this === module.exports
  };
};

exports.echo = function (v) {
  return v;
};

exports.multi = function (a, b) {
  return {
    a: a,
    b: b,
    n: arguments.length,
    thisIsExports: this === exports
  };
};

exports.boom = function (kind) {
  if (kind === "error") {
    throw new Error("zzemuC7-boom");
  }
  if (kind === "string") {
    throw "zzemuC7-string";
  }
  throw new Error("zzemuC7-other");
};
