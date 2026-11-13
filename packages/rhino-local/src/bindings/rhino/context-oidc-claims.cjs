// Overlay for AM's OIDC_CLAIMS context (next-gen), appended to the
// shared prelude for a case whose `context` names it, after
// context-common.cjs, whose shared view helpers (__rhinoLocalCtx*) it
// configures with its fault prefix. Same Rhino 1.7.14
// restrictions as runtime.cjs: no let, no top-level const, no shorthand, no
// Map/Set/Symbol.
//
// Every behaviour here was measured live on 2026-11-10 against a sandbox
// tenant, through a password-grant token request and the userinfo endpoint
// (docs/api/12-script-bindings-matrix.md, "OIDC claims").
// Anything the measurement did not reach is a named harness fault
// (`oidc-claims-unmeasured/<binding>.<member>`), never a guess; see
// docs/rhino-local-gaps.md.
//
// The script's result is the value it evaluates to. The runner hands that
// completion value to __rhinoLocalHarvestOidcClaims (the job's
// `completionHarvest`), so the author's source runs with nothing appended.

var __rhinoLocalOidc = {
  seed: null,
  claims: null,
  claimsStore: null,
  claimsKnown: null,
  claimsMethods: null,
};

// Globals the context does not bind (typeof "undefined" on AIC, measured).
var __rhinoLocalOidcUnbound = [
  "nodeState", "action", "callbacksBuilder", "callbacks", "sharedState", "transientState", "requestHeaders",
  "requestParameters", "requestCookies", "existingSession", "cacheManager", "journey", "samlApplication",
  "oauthApplication", "locales", "resumedFromSuspend", "outcome", "idRepository", "accessToken",
  "session", "claimObjects", "requestedTypedClaims",
];

// Claims AM writes into the id token itself. Returning one as a value is
// unmeasured: which of the two wins was not observed.
var __rhinoLocalOidcReserved = [
  "aud", "azp", "realm", "tokenName", "tokenType", "iss", "exp", "iat", "nbf", "jti", "auth_time", "nonce", "acr",
  "amr", "at_hash", "c_hash", "s_hash", "sid", "auditTrackingId", "org.forgerock.openidconnect.ops",
];

__rhinoLocalCtx.prefix = "oidc-claims-unmeasured";
__rhinoLocalCtx.noun = "OIDC claims";

// --- The `claims` binding ------------------------------------------------------

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
  __rhinoLocalCtxBegin();
  __rhinoLocalCtxUnbind(__rhinoLocalOidcUnbound);
  var d = __rhinoLocalOidcDefaults(given);
  __rhinoLocalOidc.seed = d;
  __rhinoLocalOidc.claimsKnown = null;
  __rhinoLocalOidc.claimsMethods = null;
  if (given.realm === undefined) {
    realm = d.realm;
  }
  identity = __rhinoLocalCtxBuildIdentity(d.identity);
  scopes = __rhinoLocalCtxBuildScopes(d.scopes);
  requestProperties = __rhinoLocalCtxBuildRequest(d.request, d.realm, {
    paramsGetMissing: true,
  });
  clientProperties = __rhinoLocalCtxBuildClient(d.client, {
    redirectUris: true,
  });
  claimsLocales = __rhinoLocalCtxList(d.oidc.claimsLocales.slice());
  claims = __rhinoLocalOidcNextGenClaims(d.oidc);
  requestedClaims = __rhinoLocalCtxMap("requestedClaims", [], null, function () {
    return "{  }";
  }, null, { get: { arity: 1, fn: function () { return null; } } });
  // getName() names the decision-node logger in runtime.cjs; this
  // context's logger name is unmeasured.
  __rhinoLocalHide(logger, "getName", function () {
    throw __rhinoLocalCtxUnmeasured("logger.getName()");
  });
  __rhinoLocalOidc.claims = claims;
  __rhinoLocalCtxSeal();
}

// --- Harvest -------------------------------------------------------------------

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

// The id token's script claims: the subject rule (measured).
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
  __rhinoLocalOidcAuditClaims();
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
