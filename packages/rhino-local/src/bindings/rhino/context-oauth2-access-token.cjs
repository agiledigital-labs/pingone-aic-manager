// Overlay for AM's OAUTH2_ACCESS_TOKEN_MODIFICATION context (legacy and
// next-gen), appended to the shared prelude for a case whose `context` names
// it, after context-common.cjs, whose shared view helpers (__rhinoLocalCtx*)
// it configures with its fault prefix and list kind. Same Rhino 1.7.14
// restrictions as runtime.cjs: no let, no top-level const, no shorthand, no
// Map/Set/Symbol.
//
// Every behaviour here was measured live on 2026-11-10 against a sandbox
// tenant with a stateless client_credentials token, on both engines
// (docs/api/12-script-bindings-matrix.md, "Access-token modification").
// Anything the measurement did not reach is a named harness fault
// (`oauth2-token-unmeasured/<binding>.<member>`), never a guess; see
// docs/rhino-local-gaps.md.

var __rhinoLocalAt = {
  initial: null,
  state: null,
  refusal: null,
  scopeRemoved: false,
};

// Globals the context does not bind (typeof "undefined" on AIC, measured).
var __rhinoLocalAtUnbound = {
  both: [
    "nodeState", "action", "callbacksBuilder", "callbacks", "sharedState", "transientState", "requestHeaders",
    "requestParameters", "requestCookies", "existingSession", "cacheManager", "journey", "samlApplication",
    "oauthApplication", "locales", "resumedFromSuspend", "outcome", "idRepository", "session",
  ],
  legacy: [
    "cookieName", "secrets", "openidm", "utils", "emailService", "jwtAssertion", "jwtValidator", "policy", "require",
  ],
};

var __rhinoLocalAtClass = {
  "next-gen": "org.forgerock.oauth2.core.scripting.AccessTokenScriptWrapper",
  legacy: "org.forgerock.openam.oauth2.token.stateless.StatelessAccessToken",
};

// Claims AM writes itself. Overwriting one with setField is unmeasured.
var __rhinoLocalAtReserved = [
  "aud", "auditTrackingId", "authGrantId", "auth_time", "client_id", "cts", "exp", "expires_in", "grant_type", "iat",
  "iss", "jti", "nbf", "realm", "scope", "sub", "subname", "tokenName", "token_type", "nonce", "auth_level", "claims",
  "act", "may_act", "permissions", "cnf",
];

__rhinoLocalListKinds.oauth2Token = {
  id: "oauth2-token-unmeasured/list",
  noun: "an access-token context list",
  measured: "only size(), length, [i], String(v) and, where docs/api/12 says so, contains() and add() were measured",
  outOfRange: false,
  concatReceiver: false,
  concatArgument: false,
  json: false,
  genericReceiver: false,
};

__rhinoLocalCtx.prefix = "oauth2-token-unmeasured";
__rhinoLocalCtx.noun = "access-token modification";
__rhinoLocalCtx.listKind = __rhinoLocalListKinds.oauth2Token;

function __rhinoLocalAtSetField(name, value, method) {
  if (typeof name !== "string") {
    throw __rhinoLocalCtxUnmeasured("accessToken." + method + "()", "with a " + typeof name + " field name");
  }
  if (__rhinoLocalAtReserved.indexOf(name) !== -1) {
    throw __rhinoLocalCtxUnmeasured("accessToken." + method + "(" + JSON.stringify(name) + ", …)",
      "overwriting a claim AM sets itself");
  }
  if (value === null) {
    // Measured: setField(k, null) leaves the claim absent.
    delete __rhinoLocalAt.state.fields[name];
    return;
  }
  if (!__rhinoLocalCtxJsonSafe(value, false)) {
    throw __rhinoLocalCtxUnmeasured("accessToken." + method + "()",
      "with a value that is not a string, finite number, boolean, array or plain object (or one holding null)");
  }
  Object.defineProperty(__rhinoLocalAt.state.fields, name, {
    value: __rhinoLocalCtxCopy(value), enumerable: true, configurable: true, writable: true,
  });
}

function __rhinoLocalAtScopeFromArgument(value) {
  var out = [];
  var i;
  var it;
  var item;
  if (__rhinoLocalCtx.engine === "legacy") {
    if (Array.isArray(value)) {
      throw __rhinoLocalInternalError("Cannot convert org.mozilla.javascript.NativeArray@" +
        "rhino-local to java.util.Set");
    }
    if (typeof java !== "undefined" && value instanceof java.util.Set) {
      it = value.iterator();
      while (it.hasNext()) {
        item = it.next();
        if (!(item instanceof java.lang.String)) {
          throw __rhinoLocalCtxUnmeasured("accessToken.setScope()", "with a set holding a non-string");
        }
        out.push(String(item));
      }
      return out;
    }
    throw __rhinoLocalCtxUnmeasured("accessToken.setScope(" + __rhinoLocalArgShape([value]) + ")");
  }
  var row = __rhinoLocalListRow(value);
  var items = row !== null ? row.items : Array.isArray(value) ? value : null;
  if (items === null) {
    throw __rhinoLocalCtxUnmeasured("accessToken.setScope(" + __rhinoLocalArgShape([value]) + ")");
  }
  for (i = 0; i < items.length; i += 1) {
    if (typeof items[i] !== "string") {
      throw __rhinoLocalCtxUnmeasured("accessToken.setScope()", "with a non-string scope");
    }
    if (out.indexOf(items[i]) === -1) {
      out.push(items[i]);
    }
  }
  return out;
}

function __rhinoLocalAtTokenState(seed) {
  return {
    fields: seed.fields,
    responseFields: {},
    scope: seed.scope,
    expiryTime: seed.expiryTime,
    nonce: seed.nonce,
    authLevel: seed.authLevel,
    claims: seed.claims,
    act: null,
    mayAct: null,
    permissions: null,
  };
}

function __rhinoLocalAtBuildToken(seed) {
  var legacy = __rhinoLocalCtx.engine === "legacy";
  var cls = __rhinoLocalAtClass[__rhinoLocalCtx.engine];
  var token = {};
  var state = function () {
    return __rhinoLocalAt.state;
  };
  var m = function (name, arity, fn) {
    // Measured 2026-11-10: setField/getField at the wrong count are Rhino's
    // ordinary overload refusal. Every other count is a named fault.
    __rhinoLocalCtxMethod(token, "accessToken", cls, name, arity, fn, name === "setField" || name === "getField");
  };
  // Getters a script cannot change.
  m("getRealm", 0, function () {
    return __rhinoLocalCtxString(seed.realm);
  });
  m("getClientId", 0, function () {
    return __rhinoLocalCtxString(seed.clientId);
  });
  m("getResourceOwnerId", 0, function () {
    return __rhinoLocalCtxString(seed.resourceOwnerId);
  });
  m("getGrantType", 0, function () {
    return __rhinoLocalCtxString(seed.grantType);
  });
  m("getTokenType", 0, function () {
    return __rhinoLocalCtxString(seed.tokenType);
  });
  m("getTokenName", 0, function () {
    return __rhinoLocalCtxString(seed.tokenName);
  });
  m("getTokenId", 0, function () {
    return __rhinoLocalCtxString(seed.tokenId);
  });
  m("getAuditTrackingId", 0, function () {
    return __rhinoLocalCtxString(seed.auditTrackingId);
  });
  m("getAuthGrantId", 0, function () {
    return __rhinoLocalCtxString(seed.authGrantId);
  });
  m("getAuthTimeSeconds", 0, function () {
    return seed.authTimeSeconds;
  });
  m("getAudience", 0, function () {
    return __rhinoLocalCtxList(seed.audience.map(__rhinoLocalCtxString));
  });
  m("getExpiryTime", 0, function () {
    return state().expiryTime;
  });
  m("isExpired", 0, function () {
    if (state().expiryTime <= Date.now()) {
      throw __rhinoLocalCtxUnmeasured("accessToken.isExpired()", "on a token whose expiry has passed");
    }
    return false;
  });
  m("getScope", 0, function () {
    if (__rhinoLocalAt.scopeRemoved) {
      throw __rhinoLocalCtxUnmeasured("accessToken.getScope()", "after removeScopes()");
    }
    if (legacy) {
      // A copy: add() answers true and the token keeps its scope (measured).
      return __rhinoLocalCtxJavaSet(state().scope, "accessToken.getScope()", true);
    }
    var list = __rhinoLocalCtxList(state().scope.slice(), "accessToken.getScope()");
    // Measured: the next-gen list is immutable, and add() throws "null".
    __rhinoLocalHide(list, "add", function () {
      throw __rhinoLocalInternalError("null");
    });
    __rhinoLocalCtxRewatch(list);
    return list;
  });
  m("setScope", 1, function (value) {
    state().scope = __rhinoLocalAtScopeFromArgument(value);
    __rhinoLocalAt.scopeRemoved = false;
    return undefined;
  });
  m("removeScopes", 0, function () {
    // Measured on both engines: the script returns, and AM then fails the
    // token request with a 500 server_error.
    __rhinoLocalAt.scopeRemoved = true;
    __rhinoLocalAt.refusal = 'Cannot invoke "java.util.Collection.size()" because "c" is null';
    state().scope = [];
    return undefined;
  });
  m("setExpiryTime", 1, function (ms) {
    if (typeof ms !== "number" || ms % 1 !== 0) {
      throw __rhinoLocalCtxUnmeasured("accessToken.setExpiryTime(" + __rhinoLocalArgShape(arguments) + ")");
    }
    state().expiryTime = ms;
    return undefined;
  });
  m("setField", 2, function (name, value) {
    __rhinoLocalAtSetField(name, value, "setField");
    return undefined;
  });
  m("setFields", 1, function (fields) {
    var keys;
    var i;
    if (!__rhinoLocalCtxIsJsonObject(fields)) {
      throw __rhinoLocalCtxUnmeasured("accessToken.setFields(" + __rhinoLocalArgShape(arguments) + ")");
    }
    keys = Object.keys(fields);
    for (i = 0; i < keys.length; i += 1) {
      if (fields[keys[i]] === null) {
        throw __rhinoLocalCtxUnmeasured("accessToken.setFields()", "with a null value");
      }
    }
    for (i = 0; i < keys.length; i += 1) {
      __rhinoLocalAtSetField(keys[i], fields[keys[i]], "setFields");
    }
    return undefined;
  });
  m("getField", 1, function (name) {
    if (typeof name !== "string") {
      throw __rhinoLocalCtxUnmeasured("accessToken.getField(" + __rhinoLocalArgShape(arguments) + ")");
    }
    var value = Object.prototype.hasOwnProperty.call(state().fields, name) ? __rhinoLocalClone(state().fields[name]) : null;
    return legacy ? __rhinoLocalCtxJsonValue(value) : value;
  });
  m("getCustomFields", 0, function () {
    var fields = state().fields;
    var untouched = JSON.stringify(fields) === JSON.stringify(__rhinoLocalAt.initial.fields);
    return __rhinoLocalCtxMap("accessToken.getCustomFields()", Object.keys(fields), function () {
      throw __rhinoLocalCtxUnmeasured("accessToken.getCustomFields()", "entry values");
    }, untouched ? function () {
      return __rhinoLocalCtxRenderMap(fields, legacy);
    } : null);
  });
  m("setNonce", 1, function (value) {
    if (typeof value !== "string") {
      throw __rhinoLocalCtxUnmeasured("accessToken.setNonce(" + __rhinoLocalArgShape(arguments) + ")");
    }
    state().nonce = value;
    return undefined;
  });
  m("getNonce", 0, function () {
    return state().nonce === null ? null : __rhinoLocalCtxString(state().nonce);
  });
  m("setClaims", 1, function (value) {
    if (typeof value !== "string") {
      throw __rhinoLocalCtxUnmeasured("accessToken.setClaims(" + __rhinoLocalArgShape(arguments) + ")");
    }
    state().claims = value;
    return undefined;
  });
  m("getClaims", 0, function () {
    return state().claims === null ? null : __rhinoLocalCtxString(state().claims);
  });
  m("setAuthLevel", 1, function (value) {
    if (typeof value !== "number" || value % 1 !== 0) {
      throw __rhinoLocalCtxUnmeasured("accessToken.setAuthLevel(" + __rhinoLocalArgShape(arguments) + ")");
    }
    state().authLevel = value;
    return undefined;
  });
  m("getAuthLevel", 0, function () {
    var level = state().authLevel;
    if (level === null) {
      return null;
    }
    return legacy ? __rhinoLocalCtxJavaNumber(level) : level;
  });
  m("addExtraData", 2, function (name, value) {
    if (typeof name !== "string" || typeof value !== "string") {
      throw __rhinoLocalCtxUnmeasured("accessToken.addExtraData(" + __rhinoLocalArgShape(arguments) + ")");
    }
    Object.defineProperty(state().responseFields, name, { value: value, enumerable: true, configurable: true, writable: true });
    return undefined;
  });
  ["setAct", "setMayAct", "setPermissions"].forEach(function (name) {
    var key = name === "setAct" ? "act" : name === "setMayAct" ? "mayAct" : "permissions";
    m(name, 1, function (value) {
      if (legacy) {
        throw __rhinoLocalCtxNoMethod(cls, name, arguments);
      }
      if (!__rhinoLocalCtxIsJsonObject(value) || !__rhinoLocalCtxJsonSafe(value, false)) {
        throw __rhinoLocalCtxUnmeasured("accessToken." + name + "(" + __rhinoLocalArgShape(arguments) + ")");
      }
      state()[key] = __rhinoLocalCtxCopy(value);
      return undefined;
    });
  });
  m("addExtraJsonData", 2, function (name, value) {
    // Measured on legacy with two strings only (2026-11-10): no overload takes
    // them. docs/api/12's 2026-08-27 list calls it working, without argument
    // types, so any other legacy shape is unmeasured rather than refused.
    if (legacy && typeof name === "string" && typeof value === "string") {
      throw __rhinoLocalCtxNoMethod(cls, "addExtraJsonData", arguments);
    }
    if (legacy || typeof name !== "string" || typeof value !== "string") {
      throw __rhinoLocalCtxUnmeasured("accessToken.addExtraJsonData(" + __rhinoLocalArgShape(arguments) + ")");
    }
    // Measured: the response body carries the string as given, not parsed.
    Object.defineProperty(state().responseFields, name, { value: value, enumerable: true, configurable: true, writable: true });
    return undefined;
  });
  ["getAct", "getMayAct", "getPermissions", "getConfirmationKey"].forEach(function (name) {
    var key = name === "getAct" ? "act" : name === "getMayAct" ? "mayAct" : name === "getPermissions" ? "permissions" : null;
    m(name, 0, function () {
      if (key !== null && state()[key] !== null) {
        throw __rhinoLocalCtxUnmeasured("accessToken." + name + "()", "after it was set");
      }
      return legacy ? __rhinoLocalCtxJsonValue(null) : null;
    });
  });
  m("getResourceOwner", 0, function () {
    if (legacy) {
      throw __rhinoLocalShutterError("org.forgerock.oauth2.core.ResourceOwner");
    }
    throw __rhinoLocalCtxUnmeasured("accessToken.getResourceOwner()", "(a ResourceOwnerScriptWrapper on AIC)");
  });
  __rhinoLocalCtxFaulting(token, "accessToken", [
    "toMap", "getTokenInfo", "removeAuditTrackingId", "removeAuthGrantId", "removeAuthLevel", "removeAuthTime",
    "removeClaims", "removeClientId", "removeConfirmationKey", "removeGrantType", "removeNonce", "removePermissions",
    "removeRealm", "removeResourceOwnerId", "removeTokenName", "removeTokenType", "setAuditTrackingId",
    "setAuthGrantId", "setAuthTime", "setClientId", "setConfirmationKey", "setGrantType", "setId", "setRealm",
    "setResourceOwnerId", "setTokenName", "setTokenType",
  ].concat(__rhinoLocalCtxObjectMethods));
  __rhinoLocalCtxToJSON(token, "accessToken");
  // Its Java tag is unmeasured on both engines (Sol ctx r2 P2-5).
  __rhinoLocalCtxTag(token, null, "accessToken");
  return __rhinoLocalCtxWatch(token, "accessToken");
}

function __rhinoLocalAtDefaults(given) {
  var at = given.accessToken || {};
  var clientId = at.clientId !== undefined ? at.clientId : "test-client";
  var owner = at.resourceOwnerId !== undefined ? at.resourceOwnerId : clientId;
  var tokenRealm = at.realm !== undefined ? at.realm : given.realm !== undefined ? given.realm : "/alpha";
  var auth = at.authTimeSeconds !== undefined ? at.authTimeSeconds : Math.floor(Date.now() / 1000);
  var scope = at.scope !== undefined ? at.scope.slice() : [];
  var token = {
    clientId: clientId,
    resourceOwnerId: owner,
    grantType: at.grantType !== undefined ? at.grantType : "client_credentials",
    realm: tokenRealm,
    scope: scope,
    audience: at.audience !== undefined ? at.audience.slice() : [clientId],
    tokenName: at.tokenName !== undefined ? at.tokenName : "access_token",
    tokenType: at.tokenType !== undefined ? at.tokenType : "Bearer",
    authTimeSeconds: auth,
    expiryTime: at.expiryTime !== undefined ? at.expiryTime : auth * 1000 + 3600000,
    fields: at.fields !== undefined ? __rhinoLocalCtxCopy(at.fields) : { subname: owner, expires_in: 3599 },
    nonce: at.nonce !== undefined ? at.nonce : null,
    authLevel: at.authLevel !== undefined ? at.authLevel : null,
    claims: at.claims !== undefined ? at.claims : null,
    tokenId: at.tokenId !== undefined ? at.tokenId : "rhino-local-token-id",
    auditTrackingId: at.auditTrackingId !== undefined ? at.auditTrackingId : "rhino-local-audit-tracking-id",
    authGrantId: at.authGrantId !== undefined ? at.authGrantId : "rhino-local-auth-grant-id",
  };
  var id = given.identity || {};
  var name = id.name !== undefined ? id.name : owner;
  var bareRealm = String(tokenRealm).replace(/^\//, "") || "root";
  var identity = {
    name: name,
    universalId: id.universalId !== undefined
      ? id.universalId
      : "id=" + name + ",ou=agentonly,o=" + bareRealm + ",ou=services,ou=am-config",
    exists: id.exists !== undefined ? id.exists : true,
    attributes: id.attributes !== undefined ? __rhinoLocalCtxCopy(id.attributes) : {},
  };
  var c = given.client || {};
  var client = {
    clientId: c.clientId !== undefined ? c.clientId : clientId,
    allowedGrantTypes: c.allowedGrantTypes !== undefined ? c.allowedGrantTypes.slice() : ["CLIENT_CREDENTIALS"],
    allowedScopes: c.allowedScopes !== undefined ? c.allowedScopes.slice() : scope.slice(),
    allowedResponseTypes: c.allowedResponseTypes !== undefined ? c.allowedResponseTypes.slice() : [],
    customProperties: c.customProperties !== undefined ? __rhinoLocalCtxCopy(c.customProperties) : {},
    redirectUris: c.redirectUris !== undefined ? c.redirectUris.slice() : [],
  };
  var r = given.request || {};
  var params = {
    grant_type: [token.grantType],
    scope: [scope.join(" ")],
    client_id: [clientId],
  };
  if (scope.length === 0) {
    delete params.scope;
  }
  var request = {
    params: r.params !== undefined ? __rhinoLocalCtxCopy(r.params) : params,
    headers: r.headers !== undefined ? __rhinoLocalCtxCopy(r.headers) : {},
    uri: r.uri !== undefined
      ? r.uri
      : "https://tenant.example.com/am/oauth2/realms/root/realms/" + bareRealm + "/access_token",
  };
  return {
    token: token,
    identity: identity,
    client: client,
    request: request,
    scopes: given.scopes !== undefined ? given.scopes.slice() : scope.slice(),
  };
}

// Runs after __rhinoLocalSeed, on the same seed object.
function __rhinoLocalSeedAccessToken(given) {
  given = given || {};
  __rhinoLocalCtxBegin(given.engine);
  var engine = __rhinoLocalCtx.engine;
  // Legacy strings are real java.lang.String objects, as in OIDC claims:
  // measured 2026-11-10, `identity.getName() == identity.getName()` is true
  // inside a function, which two string views cannot reproduce
  // (context-common.cjs, __rhinoLocalCtxJavaString).
  __rhinoLocalCtx.realJavaStrings = engine === "legacy";
  var unbound = __rhinoLocalAtUnbound.both.concat(engine === "legacy" ? __rhinoLocalAtUnbound.legacy : []);
  var i;
  for (i = 0; i < unbound.length; i += 1) {
    delete __rhinoLocalTop[unbound[i]];
  }
  var d = __rhinoLocalAtDefaults(given);
  __rhinoLocalAt.refusal = null;
  __rhinoLocalAt.scopeRemoved = false;
  __rhinoLocalAt.state = __rhinoLocalAtTokenState(d.token);
  __rhinoLocalAt.initial = __rhinoLocalClone(__rhinoLocalAt.state);
  if (given.realm === undefined) {
    realm = d.token.realm;
  }
  accessToken = __rhinoLocalAtBuildToken(d.token);
  identity = __rhinoLocalCtxBuildIdentity(d.identity);
  scopes = __rhinoLocalCtxBuildScopes(d.scopes, { add: true });
  requestProperties = __rhinoLocalCtxBuildRequest(d.request, d.token.realm);
  clientProperties = __rhinoLocalCtxBuildClient(d.client, null);
  if (engine === "legacy") {
    // Measured: bound, and null.
    session = null;
  }
  __rhinoLocalCtxSeal();
}

function __rhinoLocalHarvestAccessToken() {
  __rhinoLocalCtxAudit();
  var effects = JSON.parse(__rhinoLocalHarvest());
  // The context decides no outcome; a script that assigns `outcome` assigns
  // an ordinary variable.
  effects.outcome = null;
  effects.discardedOutcome = null;
  effects.accessToken = {
    initial: __rhinoLocalAt.initial,
    final: __rhinoLocalClone(__rhinoLocalAt.state),
    refusal: __rhinoLocalAt.refusal,
  };
  return JSON.stringify(effects);
}

var accessToken;
var identity;
var scopes;
var requestProperties;
var clientProperties;
