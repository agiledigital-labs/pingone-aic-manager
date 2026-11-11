// Overlay for AM's OIDC_CLAIMS context (legacy and next-gen), appended to the
// shared prelude for a case whose `context` names it, after
// context-common.cjs, whose shared view helpers (__rhinoLocalCtx*) it
// configures with its fault prefix and list kind. Same Rhino 1.7.14
// restrictions as runtime.cjs: no let, no top-level const, no shorthand, no
// Map/Set/Symbol.
//
// Every behaviour here was measured live on 2026-11-10 against a sandbox
// tenant, through a password-grant token request and the userinfo endpoint,
// on both engines (docs/api/12-script-bindings-matrix.md, "OIDC claims").
// Anything the measurement did not reach is a named harness fault
// (`oidc-claims-unmeasured/<binding>.<member>`), never a guess; see
// docs/rhino-local-gaps.md.
//
// The script's result is the value it evaluates to. The runner hands that
// completion value to __rhinoLocalHarvestOidcClaims (the job's
// `completionHarvest`), so the author's source runs with nothing appended.

var __rhinoLocalOidc = {
  seed: null,
  // Every UserInfoClaims the legacy script constructed, so the harvest can
  // tell one from a lookalike object.
  built: [],
  claims: null,
  claimsStore: null,
  claimsKnown: null,
  claimsMethods: null,
};

// Globals the context does not bind (typeof "undefined" on AIC, measured).
var __rhinoLocalOidcUnbound = {
  both: [
    "nodeState", "action", "callbacksBuilder", "callbacks", "sharedState", "transientState", "requestHeaders",
    "requestParameters", "requestCookies", "existingSession", "cacheManager", "journey", "samlApplication",
    "oauthApplication", "locales", "resumedFromSuspend", "outcome", "idRepository", "accessToken",
  ],
  legacy: [
    "cookieName", "secrets", "openidm", "utils", "emailService", "jwtAssertion", "jwtValidator", "policy", "require",
  ],
  "next-gen": ["session", "claimObjects", "requestedTypedClaims"],
};

// Claims AM writes into the id token itself. Returning one as a value is
// unmeasured: which of the two wins was not observed.
var __rhinoLocalOidcReserved = [
  "aud", "azp", "realm", "tokenName", "tokenType", "iss", "exp", "iat", "nbf", "jti", "auth_time", "nonce", "acr",
  "amr", "at_hash", "c_hash", "s_hash", "sid", "auditTrackingId", "org.forgerock.openidconnect.ops",
];

__rhinoLocalListKinds.oidcClaims = {
  id: "oidc-claims-unmeasured/list",
  noun: "an OIDC claims context list",
  measured: "only size(), length, [i], String(v) and, where docs/api/12 says so, contains() and toArray() were measured",
  outOfRange: false,
  concatReceiver: false,
  concatArgument: false,
  json: false,
  genericReceiver: false,
  // No binding of this kind is granted an empty list by the kind (Sol ctx
  // r11 P2-1). claimsLocales, redirectUris, a missing attribute and legacy
  // requestedTypedClaims each opt in on their own list. scopes was captured
  // non-empty (size 4), so an empty contains() faults. json and concat were
  // not measured at any length. A request value is oidcClaimsRequest. An
  // empty scopes.toArray() is oidcClaimsArray, not this kind.
  emptyRoutes: {},
};

// scopes.toArray() of a size-zero scopes list, and requestedTypedClaims.toArray()
// of the empty legacy list. The OIDC surface reads scopes.toArray().length on
// four scopes, which stays a plain array. Nothing measured an empty
// scopes.toArray(). requested.legacy.json `typed` reads the length of the
// empty requestedTypedClaims.toArray() and does not read String or JSON, so
// that one list opts in with length only. json is true so an ungranted empty
// JSON.stringify uses the empty-route sentence (Sol ctx r12 P2-1).
__rhinoLocalListKinds.oidcClaimsArray = {
  id: "oidc-claims-unmeasured/list.toArray()",
  noun: "an OIDC toArray() result",
  measured: "an empty scopes.toArray() result was not measured (the surface capture read length of four scopes)",
  outOfRange: false,
  concatReceiver: false,
  concatArgument: false,
  json: true,
  genericReceiver: false,
  emptyRoutes: {},
};

// A requestProperties value. The surface probe reads String, length, [i] and
// size() of grant_type, which has one element. No capture shows a size-zero
// request value. emptyRoutes is present and empty so that route faults as
// unmeasured, and not with the decision-node request list's "[\"\"]" sentence.
__rhinoLocalListKinds.oidcClaimsRequest = {
  id: "oidc-claims-unmeasured/requestProperties",
  noun: "an OIDC claims request value",
  measured: "only length, size(), [i] and String(v) of a non-empty value were measured",
  outOfRange: false,
  concatReceiver: false,
  concatArgument: false,
  json: false,
  genericReceiver: false,
  emptyRoutes: {},
};

__rhinoLocalCtx.prefix = "oidc-claims-unmeasured";
__rhinoLocalCtx.catalog = "oidc";
__rhinoLocalCtx.noun = "OIDC claims";
__rhinoLocalCtx.listKind = __rhinoLocalListKinds.oidcClaims;

// --- The `claims` binding ------------------------------------------------------

// Legacy: a real java.util.LinkedHashMap of Java strings. With the context
// factory's Java map access, member reads, for...in, put/get/remove,
// JSON.stringify and String(v) all behave as measured, because they are the
// JVM's own. Its other Java members (clear, size, putAll, ...) and member
// assignment were never measured, so the map is handed over behind
// java/ContextJavaGuard.java's gate (Sol ctx r2 P2-6).
// size() is the surface probe's (size=2); the rest are claimsmap/sideeffect's.
var __rhinoLocalOidcLegacyClaimsMethods = { get: 1, containsKey: 1, put: 2, remove: 1, size: 0 };

function __rhinoLocalOidcLegacyClaims(seed) {
  var map = new java.util.LinkedHashMap();
  map.put("sub", seed.sub);
  map.put("subname", seed.subname);
  return __rhinoLocalCtxGuardMap(map, __rhinoLocalOidcLegacyClaimsMethods, "claims");
}

var __rhinoLocalOidcClaimsFaulting = [
  "containsValue", "entrySet", "keySet", "values", "isEmpty", "putAll", "clear", "forEach", "getOrDefault",
  "getClass", "hashCode", "equals",
];

// Next-gen: the context hands over a JS-friendly map (measured): entries are
// plain JS strings, enumerable, `in` and hasOwnProperty see them, assignment
// adds one, and put/get/remove/containsKey/size answer as Java's would.
// JSON.stringify and String(v) were measured on the seeded {sub, subname}
// only, so any other content faults there. `delete` answers false and keeps
// the key on AIC, which a JS object cannot do while remove() works, so the
// harvest faults a key that vanished without remove().
function __rhinoLocalOidcNextGenClaims(seed) {
  var claims = {};
  var known = ["sub", "subname"];
  var methods = {};
  var hide = function (name, arity, fn) {
    __rhinoLocalCtxMethod(claims, "claims", "java.util.Map", name, arity, function () {
      sync();
      return fn.apply(this, arguments);
    });
    methods[name] = claims[name];
  };
  var define = function (key, value) {
    Object.defineProperty(claims, key, { value: value, enumerable: true, configurable: true, writable: true });
  };
  var own = function (key) {
    return Object.prototype.hasOwnProperty.call(claims, key) && Object.getOwnPropertyDescriptor(claims, key).enumerable;
  };
  // Keys reach the map two ways, put() and plain assignment, and only put()
  // can record its key as it happens. So every method first takes in keys
  // assigned since, and faults a known key that is gone: AIC answers false to
  // `delete` and keeps the key (Sol ctx r2 P1-1).
  var sync = function () {
    var keys = Object.keys(claims);
    var i;
    for (i = 0; i < known.length; i += 1) {
      if (!own(known[i])) {
        throw __rhinoLocalCtxUnmeasured("claims." + known[i],
          "(delete on the claims map; AIC answers false and keeps the key, use remove())");
      }
    }
    for (i = 0; i < keys.length; i += 1) {
      if (known.indexOf(keys[i]) === -1) {
        known.push(keys[i]);
      }
    }
  };
  var seeded = function () {
    var keys = Object.keys(claims);
    return keys.length === 2 && keys[0] === "sub" && keys[1] === "subname" &&
      claims.sub === seed.sub && claims.subname === seed.subname;
  };
  define("sub", seed.sub);
  define("subname", seed.subname);
  hide("get", 1, function (key) {
    var k = String(key);
    return own(k) ? claims[k] : null;
  });
  hide("containsKey", 1, function (key) {
    return own(String(key));
  });
  hide("size", 0, function () {
    return Object.keys(claims).length;
  });
  hide("put", 2, function (key, value) {
    if (typeof key !== "string") {
      throw __rhinoLocalCtxUnmeasured("claims.put(" + __rhinoLocalArgShape(arguments) + ")");
    }
    if (own(key)) {
      throw __rhinoLocalCtxUnmeasured("claims.put(" + JSON.stringify(key) + ", …)", "over a key the map holds");
    }
    define(key, value);
    if (known.indexOf(key) === -1) {
      known.push(key);
    }
    return null;
  });
  hide("remove", 1, function (key) {
    var k = String(key);
    if (!own(k)) {
      throw __rhinoLocalCtxUnmeasured("claims.remove()", "of a key the map does not hold");
    }
    var value = claims[k];
    var at = known.indexOf(k);
    delete claims[k];
    if (at !== -1) {
      known.splice(at, 1);
    }
    return value;
  });
  __rhinoLocalHide(claims, "toString", function () {
    sync();
    if (!seeded()) {
      throw __rhinoLocalCtxUnmeasured("String(claims)", "after it changed");
    }
    return '{ "sub": ' + JSON.stringify(seed.sub) + ', "subname": ' + JSON.stringify(seed.subname) + " }";
  });
  methods.toString = claims.toString;
  __rhinoLocalHide(claims, "toJSON", function () {
    sync();
    if (!seeded()) {
      throw __rhinoLocalCtxUnmeasured("JSON.stringify(claims)", "after it changed");
    }
    // Measured: Java hash order, subname first.
    return { subname: seed.subname, sub: seed.sub };
  });
  methods.toJSON = claims.toJSON;
  __rhinoLocalCtxFaulting(claims, "claims", __rhinoLocalOidcClaimsFaulting);
  __rhinoLocalOidcClaimsFaulting.forEach(function (name) {
    methods[name] = claims[name];
  });
  __rhinoLocalCtxTag(claims, null, "claims");
  methods[__rhinoLocalTagKey] = claims[__rhinoLocalTagKey];
  __rhinoLocalOidc.claimsKnown = known;
  __rhinoLocalOidc.claimsMethods = methods;
  return claims;
}

// Faults the next-gen claims view's changes no capture covered.
function __rhinoLocalOidcAuditClaims() {
  var claims = __rhinoLocalOidc.claims;
  var known = __rhinoLocalOidc.claimsKnown;
  var methods = __rhinoLocalOidc.claimsMethods;
  var i;
  var names;
  var own;
  if (known === null) {
    return;
  }
  for (i = 0; i < known.length; i += 1) {
    if (!Object.prototype.hasOwnProperty.call(claims, known[i])) {
      __rhinoLocalCtxUnmeasured("claims." + known[i],
        "(delete on the claims map; AIC answers false and keeps the key, use remove())");
    }
  }
  names = Object.keys(methods);
  for (i = 0; i < names.length; i += 1) {
    own = Object.getOwnPropertyDescriptor(claims, names[i]);
    if (own === undefined || !("value" in own) || own.value !== methods[names[i]] || own.enumerable) {
      __rhinoLocalCtxUnmeasured("claims." + names[i], "(assigning or redefining a member of the Java map)");
    }
  }
}

// --- Legacy-only bindings ----------------------------------------------------------

// A Claim in the legacy claimObjects list: measured getName(), getValues(),
// isEssential(), getLocale(), getNameWithLocale() and String(v).
function __rhinoLocalOidcClaimObject(name, value) {
  var claim = {};
  var cls = "org.forgerock.openidconnect.Claim";
  var label = "claimObjects[" + name + "]";
  __rhinoLocalCtxMethod(claim, label, cls, "getName", 0, function () {
    return __rhinoLocalCtxJavaString(name);
  });
  __rhinoLocalCtxMethod(claim, label, cls, "getNameWithLocale", 0, function () {
    return __rhinoLocalCtxJavaString(name);
  });
  __rhinoLocalCtxMethod(claim, label, cls, "getValues", 0, function () {
    return __rhinoLocalCtxList([__rhinoLocalCtxJavaString(value)], label + ".getValues()", null, "claimValues");
  });
  __rhinoLocalCtxMethod(claim, label, cls, "isEssential", 0, function () {
    return false;
  });
  __rhinoLocalCtxMethod(claim, label, cls, "getLocale", 0, function () {
    return null;
  });
  __rhinoLocalHide(claim, "toString", function () {
    return name + " -> {values: [" + value + "], essential: false}";
  });
  __rhinoLocalCtxFaulting(claim, label, ["getClass", "hashCode", "equals", "wait", "notify", "notifyAll"]);
  __rhinoLocalCtxToJSON(claim, label);
  __rhinoLocalCtxTag(claim, null, label);
  return __rhinoLocalCtxWatch(claim, label);
}

function __rhinoLocalOidcClaimObjects(seed) {
  var items = [__rhinoLocalOidcClaimObject("sub", seed.sub), __rhinoLocalOidcClaimObject("subname", seed.subname)];
  var list = __rhinoLocalCtxList(items, "claimObjects", null, "claimObjects");
  __rhinoLocalCtxMethod(list, "claimObjects", "java.util.List", "toArray", 0, function () {
    return items.slice();
  });
  return __rhinoLocalCtxWatch(list, "claimObjects");
}

// The legacy logger is AM's Debug, not slf4j (measured): message, warning,
// error, the *Enabled checks and getName exist; info/warn/debug/trace and
// isDebugEnabled do not, so calling one is Rhino's own "Cannot find function".
function __rhinoLocalOidcLegacyLogger() {
  var log = {};
  var level = function (name, recorded) {
    __rhinoLocalCtxMethod(log, "logger", "com.sun.identity.shared.debug.Debug", name, 1, function (message) {
      __rhinoLocal.logs.push({ level: recorded, message: String(message) });
      return undefined;
    });
  };
  level("message", "debug");
  level("warning", "warn");
  level("error", "error");
  __rhinoLocalCtxMethod(log, "logger", "com.sun.identity.shared.debug.Debug", "warningEnabled", 0, function () {
    return true;
  });
  __rhinoLocalCtxFaulting(log, "logger", ["messageEnabled", "errorEnabled", "getName"]);
  return log;
}

// `new org.forgerock.oauth2.core.UserInfoClaims(values, compositeScopes)`: the
// legacy result type. Read at harvest, as AM reads it after the script.
function __rhinoLocalOidcUserInfoClaims(values, compositeScopes) {
  if (arguments.length !== 2) {
    throw __rhinoLocalCtxUnmeasured("new UserInfoClaims(" + __rhinoLocalArgShape(arguments) + ")");
  }
  if (!(values instanceof java.util.Map)) {
    throw __rhinoLocalCtxUnmeasured("new UserInfoClaims(" + __rhinoLocalArgShape(arguments) + ")",
      "with values that are not a Java map");
  }
  if (compositeScopes !== null && !(compositeScopes instanceof java.util.Map)) {
    throw __rhinoLocalCtxUnmeasured("new UserInfoClaims(" + __rhinoLocalArgShape(arguments) + ")",
      "with compositeScopes that are neither null nor a Java map");
  }
  var made = { values: __rhinoLocalCtxUnguard(values), compositeScopes: __rhinoLocalCtxUnguard(compositeScopes) };
  __rhinoLocalHide(made, "toString", function () {
    throw __rhinoLocalCtxUnmeasured("String(UserInfoClaims)");
  });
  __rhinoLocalCtxFaulting(made, "UserInfoClaims", ["getValues", "getCompositeScopes", "getClass", "hashCode", "equals"]);
  __rhinoLocalCtxTag(made, null, "UserInfoClaims");
  __rhinoLocalOidc.built.push(made);
  return made;
}

// Measured: thrown from the script, the token request fails with 500
// "Error running OIDC claims plugin", like any other throw.
function __rhinoLocalOidcInvalidRequest(message) {
  if (arguments.length !== 1 || typeof message !== "string") {
    throw __rhinoLocalCtxUnmeasured("new InvalidRequestException(" + __rhinoLocalArgShape(arguments) + ")");
  }
  return __rhinoLocalJavaException("org.forgerock.oauth2.core.exceptions.InvalidRequestException: " + message);
}

// --- Seed ----------------------------------------------------------------------

function __rhinoLocalOidcDefaults(given) {
  var tokenRealm = given.realm !== undefined ? given.realm : "/alpha";
  var bareRealm = String(tokenRealm).replace(/^\//, "") || "root";
  var id = given.identity || {};
  var name = id.name !== undefined ? id.name : "rhino-local-user";
  var o = given.oidc || {};
  var sub = o.sub !== undefined ? o.sub : name;
  var scopes = given.scopes !== undefined ? given.scopes.slice() : ["openid"];
  var c = given.client || {};
  var clientId = c.clientId !== undefined ? c.clientId : "test-client";
  var r = given.request || {};
  return {
    realm: tokenRealm,
    oidc: {
      sub: sub,
      subname: o.subname !== undefined ? o.subname : sub,
      claimsLocales: o.claimsLocales !== undefined ? o.claimsLocales.slice() : [],
    },
    identity: {
      name: name,
      universalId: id.universalId !== undefined
        ? id.universalId
        : "id=" + name + ",ou=user,o=" + bareRealm + ",ou=services,ou=am-config",
      exists: id.exists !== undefined ? id.exists : true,
      attributes: id.attributes !== undefined ? __rhinoLocalCtxCopy(id.attributes) : {},
    },
    scopes: scopes,
    client: {
      clientId: clientId,
      allowedGrantTypes: c.allowedGrantTypes !== undefined
        ? c.allowedGrantTypes.slice()
        : ["RESOURCE_OWNER_PASSWORD_CREDENTIALS"],
      allowedScopes: c.allowedScopes !== undefined ? c.allowedScopes.slice() : scopes.slice(),
      allowedResponseTypes: c.allowedResponseTypes !== undefined ? c.allowedResponseTypes.slice() : [],
      customProperties: c.customProperties !== undefined ? __rhinoLocalCtxCopy(c.customProperties) : {},
      redirectUris: c.redirectUris !== undefined ? c.redirectUris.slice() : [],
    },
    request: {
      // AM filters password and client_secret out before the script sees them.
      params: r.params !== undefined ? __rhinoLocalCtxCopy(r.params) : {
        client_id: [clientId],
        grant_type: ["password"],
        scope: [scopes.join(" ")],
        username: [name],
      },
      headers: r.headers !== undefined ? __rhinoLocalCtxCopy(r.headers) : {},
      uri: r.uri !== undefined
        ? r.uri
        : "https://tenant.example.com/am/oauth2/realms/root/realms/" + bareRealm + "/access_token",
    },
  };
}

// Runs after __rhinoLocalSeed, on the same seed object.
function __rhinoLocalSeedOidcClaims(given) {
  given = given || {};
  __rhinoLocalCtxBegin(given.engine);
  var engine = __rhinoLocalCtx.engine;
  var legacy = engine === "legacy";
  // Legacy strings are real java.lang.String objects here, as the claims
  // map's own values must be (context-common.cjs, __rhinoLocalCtxJavaString).
  __rhinoLocalCtx.realJavaStrings = legacy;
  __rhinoLocalCtxUnbind(__rhinoLocalOidcUnbound.both.concat(__rhinoLocalOidcUnbound[engine]));
  var d = __rhinoLocalOidcDefaults(given);
  __rhinoLocalOidc.seed = d;
  __rhinoLocalOidc.built = [];
  __rhinoLocalOidc.claimsKnown = null;
  __rhinoLocalOidc.claimsMethods = null;
  if (given.realm === undefined) {
    realm = d.realm;
  }
  identity = __rhinoLocalCtxBuildIdentity(d.identity);
  scopes = __rhinoLocalCtxBuildScopes(d.scopes, {
    toArray: true,
    toArrayKind: __rhinoLocalListKinds.oidcClaimsArray,
  });
  requestProperties = __rhinoLocalCtxBuildRequest(d.request, d.realm, {
    paramsGetMissing: true, listKind: __rhinoLocalListKinds.oidcClaimsRequest,
  });
  clientProperties = __rhinoLocalCtxBuildClient(d.client, {
    redirectUris: true,
  });
  claimsLocales = __rhinoLocalCtxList(d.oidc.claimsLocales.map(__rhinoLocalCtxString), "claimsLocales", null, "claimsLocales");
  if (legacy) {
    claims = __rhinoLocalOidcLegacyClaims(d.oidc);
    claimObjects = __rhinoLocalOidcClaimObjects(d.oidc);
    requestedClaims = __rhinoLocalCtxMap("requestedClaims", [], null, function () {
      return "{}";
    }, null, { get: { arity: 1, fn: function () { return null; } } });
    requestedTypedClaims = __rhinoLocalCtxList([], "requestedTypedClaims", null, "requestedTypedClaims");
    // Measured: toArray() of the empty list, then a.length in a loop that does
    // not run (requested.legacy.json typed, the empty join). String and JSON
    // of that array were not read. The call itself does not fault.
    __rhinoLocalCtxMethod(requestedTypedClaims, "requestedTypedClaims", "java.util.List", "toArray", 0, function () {
      return __rhinoLocalCtxList([], "requestedTypedClaims.toArray()", __rhinoLocalListKinds.oidcClaimsArray,
        "requestedTypedClaims", "toArray");
    });
    // Measured: bound, and null on a password grant.
    session = null;
    logger = __rhinoLocalOidcLegacyLogger();
    org.forgerock.oauth2 = {
      core: {
        UserInfoClaims: __rhinoLocalOidcUserInfoClaims,
        exceptions: { InvalidRequestException: __rhinoLocalOidcInvalidRequest },
      },
    };
  } else {
    claims = __rhinoLocalOidcNextGenClaims(d.oidc);
    requestedClaims = __rhinoLocalCtxMap("requestedClaims", [], null, function () {
      return "{  }";
    }, null, { get: { arity: 1, fn: function () { return null; } } });
    // getName() names the decision-node logger in runtime.cjs; this
    // context's logger name is unmeasured.
    __rhinoLocalHide(logger, "getName", function () {
      throw __rhinoLocalCtxUnmeasured("logger.getName()");
    });
  }
  __rhinoLocalOidc.claims = claims;
  __rhinoLocalCtxSeal();
}

// --- Harvest -------------------------------------------------------------------

function __rhinoLocalOidcIsJava(value) {
  return typeof java !== "undefined" && value !== null && typeof value === "object" &&
    value instanceof java.lang.Object;
}

// One value of a legacy Java values map, as AM would serialise it.
function __rhinoLocalOidcFromJava(value, path, nested) {
  var out;
  var it;
  var entry;
  if (value === null || value === undefined) {
    if (nested || value === undefined) {
      throw __rhinoLocalCtxUnmeasured("UserInfoClaims values" + path, "(a null nested inside a value)");
    }
    return null;
  }
  if (value instanceof java.lang.String) {
    return String(value);
  }
  if (value instanceof java.lang.Boolean) {
    return value.booleanValue();
  }
  if (value instanceof java.lang.Number) {
    var n = value.doubleValue();
    if (!isFinite(n)) {
      throw __rhinoLocalCtxUnmeasured("UserInfoClaims values" + path, "(a non-finite number)");
    }
    return n;
  }
  if (value instanceof java.util.List) {
    out = [];
    it = value.iterator();
    while (it.hasNext()) {
      out.push(__rhinoLocalOidcFromJava(it.next(), path + "[]", true));
    }
    return out;
  }
  if (value instanceof java.util.Map) {
    out = {};
    it = value.entrySet().iterator();
    while (it.hasNext()) {
      entry = it.next();
      if (!(entry.getKey() instanceof java.lang.String)) {
        throw __rhinoLocalCtxUnmeasured("UserInfoClaims values" + path, "(a map with a non-string key)");
      }
      Object.defineProperty(out, String(entry.getKey()), {
        value: __rhinoLocalOidcFromJava(entry.getValue(), path + "." + entry.getKey(), true),
        enumerable: true, configurable: true, writable: true,
      });
    }
    return out;
  }
  if (__rhinoLocalOidcIsJava(value)) {
    throw __rhinoLocalCtxUnmeasured("UserInfoClaims values" + path, "(a Java " + value.getClass().getName() + ")");
  }
  if (typeof value === "string" || typeof value === "boolean") {
    return value;
  }
  if (typeof value === "number") {
    if (!isFinite(value)) {
      throw __rhinoLocalCtxUnmeasured("UserInfoClaims values" + path, "(a non-finite number)");
    }
    return value;
  }
  // A JS array or object held in the Java map: measured for plain JSON.
  if (!__rhinoLocalCtxJsonSafe(value, true)) {
    throw __rhinoLocalCtxUnmeasured("UserInfoClaims values" + path,
      "(a value that is not a string, finite number, boolean, Java list or map, or plain JSON)");
  }
  return __rhinoLocalCtxCopy(value);
}

// The values map itself: its own entries may be null (kept in userinfo,
// dropped from the id token, measured); a null deeper down is unmeasured.
function __rhinoLocalOidcLegacyValues(map) {
  var out = {};
  var it = map.entrySet().iterator();
  var entry;
  while (it.hasNext()) {
    entry = it.next();
    if (!(entry.getKey() instanceof java.lang.String)) {
      throw __rhinoLocalCtxUnmeasured("UserInfoClaims values", "(a map with a non-string key)");
    }
    Object.defineProperty(out, String(entry.getKey()), {
      value: __rhinoLocalOidcFromJava(entry.getValue(), "." + entry.getKey(), false),
      enumerable: true, configurable: true, writable: true,
    });
  }
  return out;
}

function __rhinoLocalOidcLegacyCompositeScopes(map) {
  var out;
  var it;
  var entry;
  if (map === null) {
    return null;
  }
  out = {};
  it = map.entrySet().iterator();
  while (it.hasNext()) {
    entry = it.next();
    var names = __rhinoLocalOidcFromJava(entry.getValue(), "", true);
    if (!Array.isArray(names) || names.some(function (x) { return typeof x !== "string"; })) {
      throw __rhinoLocalCtxUnmeasured("UserInfoClaims compositeScopes",
        "(a scope mapped to anything but a list of claim names)");
    }
    Object.defineProperty(out, String(entry.getKey()), {
      value: names, enumerable: true, configurable: true, writable: true,
    });
  }
  return out;
}

function __rhinoLocalOidcNextGenValues(values) {
  var out = {};
  var keys;
  var i;
  var v;
  if (values === __rhinoLocalOidc.claims) {
    keys = Object.keys(values);
  } else if (__rhinoLocalCtxIsJsonObject(values)) {
    keys = Object.keys(values);
  } else {
    throw __rhinoLocalCtxUnmeasured("result.values", "(neither a plain object nor the claims binding)");
  }
  for (i = 0; i < keys.length; i += 1) {
    v = values[keys[i]];
    if (v !== null && !__rhinoLocalCtxJsonSafe(v, true)) {
      throw __rhinoLocalCtxUnmeasured("result.values." + keys[i],
        "(a value that is not a string, finite number, boolean, array or plain object, or one holding null)");
    }
    Object.defineProperty(out, keys[i], {
      value: __rhinoLocalCtxCopy(v), enumerable: true, configurable: true, writable: true,
    });
  }
  return out;
}

function __rhinoLocalOidcNextGenCompositeScopes(value) {
  var keys;
  var i;
  if (!__rhinoLocalCtxIsJsonObject(value) || !__rhinoLocalCtxJsonSafe(value, false)) {
    throw __rhinoLocalCtxUnmeasured("result.compositeScopes", "(not a plain object)");
  }
  keys = Object.keys(value);
  for (i = 0; i < keys.length; i += 1) {
    if (!Array.isArray(value[keys[i]]) || value[keys[i]].some(function (x) { return typeof x !== "string"; })) {
      throw __rhinoLocalCtxUnmeasured("result.compositeScopes." + keys[i],
        "(a scope mapped to anything but an array of claim names)");
    }
  }
  return __rhinoLocalCtxCopy(value);
}

// What AM does with the script's value. `{returned, values, compositeScopes,
// refusal}`; a shape no capture covered faults.
function __rhinoLocalOidcResult(completion) {
  var none = { returned: "none", values: {}, compositeScopes: null, refusal: null };
  if (__rhinoLocalCtx.engine === "legacy") {
    if (completion === undefined) {
      return none;
    }
    if (__rhinoLocalOidc.built.indexOf(completion) !== -1) {
      return {
        returned: "claims",
        values: __rhinoLocalOidcLegacyValues(completion.values),
        compositeScopes: __rhinoLocalOidcLegacyCompositeScopes(completion.compositeScopes),
        refusal: null,
      };
    }
    if (__rhinoLocalOidcIsJava(completion) && completion instanceof java.util.LinkedHashMap) {
      none.refusal = "Cannot cast java.util.LinkedHashMap to org.forgerock.oauth2.core.UserInfoClaims";
      return none;
    }
    // Measured with the next-gen shape only, `{values, compositeScopes}`.
    if (completion !== null && typeof completion === "object" && !__rhinoLocalOidcIsJava(completion) &&
      __rhinoLocalCtxIsJsonObject(completion) && Object.keys(completion).sort().join() === "compositeScopes,values") {
      none.refusal = "Error running OIDC claims plugin";
      return none;
    }
    throw __rhinoLocalCtxUnmeasured("result", "(the script evaluated to " + __rhinoLocalArgShape([completion]) +
      "; only undefined, a UserInfoClaims, a LinkedHashMap and {values, compositeScopes} were measured)");
  }
  if (completion === undefined) {
    return none;
  }
  if (!__rhinoLocalCtxIsJsonObject(completion)) {
    throw __rhinoLocalCtxUnmeasured("result", "(the script evaluated to " + __rhinoLocalArgShape([completion]) +
      "; only undefined and a plain object were measured)");
  }
  var keys = Object.keys(completion);
  var hasValues = keys.indexOf("values") !== -1;
  var hasScopes = keys.indexOf("compositeScopes") !== -1;
  if (!hasValues && !hasScopes) {
    // Measured: an object with no `values` is taken as no claims.
    return none;
  }
  if (!hasValues || keys.length > (hasScopes ? 2 : 1)) {
    throw __rhinoLocalCtxUnmeasured("result", "(an object holding " + keys.join(", ") +
      "; only {values} and {values, compositeScopes} were measured)");
  }
  return {
    returned: "claims",
    values: __rhinoLocalOidcNextGenValues(completion.values),
    compositeScopes: hasScopes ? __rhinoLocalOidcNextGenCompositeScopes(completion.compositeScopes) : null,
    refusal: null,
  };
}

// The id token's script claims: the subject rule measured on both engines.
function __rhinoLocalOidcIdToken(values) {
  var seed = __rhinoLocalOidc.seed.oidc;
  var out = {};
  var keys = Object.keys(values);
  var i;
  var put = function (k, v) {
    Object.defineProperty(out, k, { value: v, enumerable: true, configurable: true, writable: true });
  };
  for (i = 0; i < keys.length; i += 1) {
    if (__rhinoLocalOidcReserved.indexOf(keys[i]) !== -1) {
      throw __rhinoLocalCtxUnmeasured("result.values." + keys[i], "(a claim AM sets in the id token itself)");
    }
  }
  if (Object.prototype.hasOwnProperty.call(values, "sub")) {
    if (typeof values.sub !== "string") {
      throw __rhinoLocalCtxUnmeasured("result.values.sub", "(a subject that is not a string)");
    }
    put("sub", values.sub);
    if (Object.prototype.hasOwnProperty.call(values, "subname") && values.subname !== null) {
      put("subname", values.subname);
    }
  } else {
    put("sub", seed.sub);
    put("subname", seed.subname);
  }
  for (i = 0; i < keys.length; i += 1) {
    if (keys[i] !== "sub" && keys[i] !== "subname" && values[keys[i]] !== null) {
      put(keys[i], values[keys[i]]);
    }
  }
  return out;
}

function __rhinoLocalHarvestOidcClaims(completion) {
  __rhinoLocalCtxAudit();
  if (__rhinoLocalCtx.engine !== "legacy") {
    __rhinoLocalOidcAuditClaims();
  }
  var result = __rhinoLocalOidcResult(completion);
  var effects = JSON.parse(__rhinoLocalHarvest());
  // The context decides no outcome; a script that assigns `outcome` assigns
  // an ordinary variable.
  effects.outcome = null;
  effects.discardedOutcome = null;
  effects.oidc = {
    returned: result.returned,
    userinfo: result.values,
    idToken: result.refusal === null ? __rhinoLocalOidcIdToken(result.values) : {},
    compositeScopes: result.compositeScopes,
    refusal: result.refusal,
  };
  return JSON.stringify(effects);
}

var claims;
var claimObjects;
var claimsLocales;
var requestedClaims;
var requestedTypedClaims;
var identity;
var scopes;
var requestProperties;
var clientProperties;
