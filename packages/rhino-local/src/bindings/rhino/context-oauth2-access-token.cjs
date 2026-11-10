// Overlay for AM's OAUTH2_ACCESS_TOKEN_MODIFICATION context (legacy and
// next-gen), appended to the shared prelude for a case whose `context` names
// it. Same Rhino 1.7.14 restrictions as runtime.cjs: no let, no top-level
// const, no shorthand, no Map/Set/Symbol.
//
// Every behaviour here was measured live on 2026-11-10 against a sandbox
// tenant with a stateless client_credentials token, on both engines
// (docs/api/12-script-bindings-matrix.md, "Access-token modification").
// Anything the measurement did not reach is a named harness fault
// (`oauth2-token-unmeasured/<binding>.<member>`), never a guess; see
// docs/rhino-local-gaps.md.

// The top-level scope, so the seed can remove a global the context does not
// bind: a script that reads one then gets AIC's ReferenceError, not undefined.
var __rhinoLocalTop = this;

var __rhinoLocalAt = {
  engine: "next-gen",
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
};

function __rhinoLocalAtUnmeasured(member, detail) {
  return __rhinoLocalFault("oauth2-token-unmeasured/" + member,
    "rhino-local: " + member + (detail ? " " + detail : "") +
      " in the access-token modification context (" + __rhinoLocalAt.engine +
      ") is unmeasured; docs/api/12-script-bindings-matrix.md lists what was measured");
}

function __rhinoLocalAtNoMethod(className, name, args) {
  return __rhinoLocalInternalError("Can't find method " + className + "." + name + "(" +
    __rhinoLocalScriptSignature(args) + ").");
}

// Install a method that answers only at its measured argument count. Any other
// count is Rhino's overload refusal, which AIC measured for setField/getField.
function __rhinoLocalAtMethod(target, className, name, arity, fn) {
  __rhinoLocalHide(target, name, function () {
    if (arguments.length !== arity) {
      throw __rhinoLocalAtNoMethod(className, name, arguments);
    }
    return fn.apply(this, arguments);
  });
}

function __rhinoLocalAtFaulting(target, label, names) {
  var i;
  for (i = 0; i < names.length; i += 1) {
    (function (name) {
      __rhinoLocalHide(target, name, function () {
        throw __rhinoLocalAtUnmeasured(label + "." + name + "()");
      });
    })(names[i]);
  }
}

var __rhinoLocalAtObjectMethods = ["toString", "getClass", "hashCode", "equals", "wait", "notify", "notifyAll"];

function __rhinoLocalAtToJSON(target, label) {
  __rhinoLocalHide(target, "toJSON", function () {
    throw __rhinoLocalAtUnmeasured("JSON.stringify(" + label + ")");
  });
}

// A java.lang.String as the legacy engine hands it over: typeof "object",
// String(v) and `"" + v` give the text, `v == "x"` is true and `v === "x"` is
// false, and `length` is a method. Java's other String methods are unmeasured.
function __rhinoLocalAtJavaString(text) {
  if (text === null || text === undefined) {
    return null;
  }
  var value = String(text);
  var out = typeof Object.create === "function" ? Object.create(null) : {};
  __rhinoLocalHide(out, "toString", function () {
    return value;
  });
  __rhinoLocalHide(out, "valueOf", function () {
    return value;
  });
  __rhinoLocalHide(out, "length", function () {
    return value.length;
  });
  __rhinoLocalAtFaulting(out, "java.lang.String", [
    "charAt", "codePointAt", "compareTo", "compareToIgnoreCase", "concat", "contains", "contentEquals", "endsWith",
    "equals", "equalsIgnoreCase", "getBytes", "getClass", "hashCode", "indexOf", "intern", "isBlank", "isEmpty",
    "lastIndexOf", "lines", "matches", "repeat", "replace", "replaceAll", "replaceFirst", "split", "startsWith",
    "strip", "substring", "toCharArray", "toLowerCase", "toUpperCase", "trim", "slice", "match", "search",
  ]);
  __rhinoLocalAtToJSON(out, "a Java string");
  return out;
}

// A legacy java.lang.Integer (setAuthLevel then getAuthLevel): typeof
// "object", String(v) gives the digits. Only that was measured.
function __rhinoLocalAtJavaNumber(n) {
  var out = typeof Object.create === "function" ? Object.create(null) : {};
  __rhinoLocalHide(out, "toString", function () {
    return String(n);
  });
  __rhinoLocalHide(out, "valueOf", function () {
    return n;
  });
  __rhinoLocalAtFaulting(out, "java.lang.Integer", [
    "intValue", "longValue", "doubleValue", "equals", "compareTo", "hashCode", "getClass",
  ]);
  __rhinoLocalAtToJSON(out, "a Java number");
  return out;
}

// The JsonValue a legacy getField / getAct hands back: typeof "object",
// member reads such as `v.a` are undefined, and String(v) is the JSON-ish
// rendering (`"hello"`, `42.0`, `{ "a": 1.0 }`). size() was measured only on a
// null value (0). Every JsonValue method is unmeasured.
function __rhinoLocalAtJsonValue(value) {
  var out = typeof Object.create === "function" ? Object.create(null) : {};
  __rhinoLocalHide(out, "toString", function () {
    if (value !== null && typeof value === "object" && __rhinoLocalAtIsEmpty(value)) {
      throw __rhinoLocalAtUnmeasured("String(JsonValue)", "of an empty object or array");
    }
    return __rhinoLocalJavaMapString(value);
  });
  __rhinoLocalHide(out, "size", function () {
    if (value === null && arguments.length === 0) {
      return 0;
    }
    throw __rhinoLocalAtUnmeasured("JsonValue.size()", "on a non-null value");
  });
  __rhinoLocalAtFaulting(out, "JsonValue", [
    "get", "getObject", "isNull", "isString", "asString", "isMap", "asMap", "isList", "asList", "isNumber",
    "asNumber", "asInteger", "asLong", "asDouble", "isBoolean", "asBoolean", "put", "add", "remove", "keys",
    "expect", "required", "defaultTo", "getPointer", "copy", "clone", "isDefined", "contains", "iterator",
    "getClass", "hashCode", "equals",
  ]);
  __rhinoLocalAtToJSON(out, "a JsonValue");
  return out;
}

function __rhinoLocalAtIsEmpty(value) {
  if (Array.isArray(value)) {
    return value.length === 0;
  }
  return Object.keys(value).length === 0;
}

// The engine's string: a JS string on next-gen, a Java string on legacy.
function __rhinoLocalAtString(text) {
  if (__rhinoLocalAt.engine === "legacy") {
    return __rhinoLocalAtJavaString(text);
  }
  return text;
}

// A next-gen JavaList (and a legacy one, where the tag was also JavaList).
function __rhinoLocalAtList(items) {
  return __rhinoLocalListView(items, __rhinoLocalListKinds.oauth2Token);
}

// A legacy java.util.HashSet: no `length`, `[i]` is AIC's InternalError
// (measured for index 0; the rest of the range assumed the same), and only
// size(), contains() and String(v) were measured, plus add() where the caller
// says so. Element order is AIC's hash order, not reproduced.
function __rhinoLocalAtJavaSet(items, label, addable) {
  var out = typeof Object.create === "function" ? Object.create(null) : {};
  var store = items.slice();
  var i;
  for (i = 0; i < store.length + 8; i += 1) {
    (function (index) {
      Object.defineProperty(out, String(index), {
        enumerable: false,
        configurable: true,
        get: function () {
          throw __rhinoLocalInternalError('Java class "java.util.HashSet" has no public instance field or method named "' +
            index + '".');
        },
      });
    })(i);
  }
  __rhinoLocalAtFaulting(out, label, [
    "addAll", "clear", "containsAll", "equals", "forEach", "getClass", "hashCode", "iterator",
    "parallelStream", "remove", "removeAll", "removeIf", "retainAll", "spliterator", "stream", "clone",
  ]);
  __rhinoLocalAtMethod(out, "java.util.HashSet", "isEmpty", 0, function () {
    return store.length === 0;
  });
  // Measured 2026-09-10 on identity.getAttribute(): toArray() resolves, and
  // String(v.toArray()[0]) is how a single-valued attribute is read.
  __rhinoLocalAtMethod(out, "java.util.HashSet", "toArray", 0, function () {
    var arr = [];
    var k;
    for (k = 0; k < store.length; k += 1) {
      arr.push(__rhinoLocalAtJavaString(store[k]));
    }
    return arr;
  });
  __rhinoLocalAtMethod(out, "java.util.HashSet", "size", 0, function () {
    return store.length;
  });
  __rhinoLocalAtMethod(out, "java.util.HashSet", "contains", 1, function (v) {
    return store.indexOf(String(v)) !== -1;
  });
  __rhinoLocalHide(out, "add", function (v) {
    if (!addable) {
      throw __rhinoLocalAtUnmeasured(label + ".add()");
    }
    if (arguments.length !== 1 || typeof v !== "string") {
      throw __rhinoLocalAtUnmeasured(label + ".add(" + __rhinoLocalArgShape(arguments) + ")");
    }
    if (store.indexOf(v) !== -1) {
      return false;
    }
    store.push(v);
    return true;
  });
  __rhinoLocalHide(out, "toString", function () {
    return "[" + store.join(", ") + "]";
  });
  __rhinoLocalAtToJSON(out, label);
  return out;
}

// A Java map as the context hands it over. Entries are enumerable accessors,
// so for...in and dot access see them; an entry whose value was not measured
// faults on read. get(k) answers like dot access, size() is measured, and
// String(v) only where `render` is given.
function __rhinoLocalAtMap(label, keys, read, render, aliases) {
  var map = {};
  var i;
  for (i = 0; i < keys.length; i += 1) {
    (function (key) {
      Object.defineProperty(map, key, {
        enumerable: true,
        configurable: true,
        get: function () {
          return read(key);
        },
      });
    })(keys[i]);
  }
  var aliasKeys = aliases ? Object.keys(aliases) : [];
  for (i = 0; i < aliasKeys.length; i += 1) {
    (function (alias) {
      if (keys.indexOf(alias) !== -1) {
        return;
      }
      Object.defineProperty(map, alias, {
        enumerable: false,
        configurable: true,
        get: function () {
          return read(aliases[alias]);
        },
      });
    })(aliasKeys[i]);
  }
  __rhinoLocalHide(map, "get", function (key) {
    if (arguments.length !== 1) {
      throw __rhinoLocalAtUnmeasured(label + ".get(" + __rhinoLocalArgShape(arguments) + ")");
    }
    var k = String(key);
    if (keys.indexOf(k) !== -1) {
      return read(k);
    }
    if (aliases && Object.prototype.hasOwnProperty.call(aliases, k)) {
      return read(aliases[k]);
    }
    throw __rhinoLocalAtUnmeasured(label + ".get()", "of a key the map does not hold");
  });
  __rhinoLocalAtMethod(map, "java.util.Map", "size", 0, function () {
    return keys.length;
  });
  __rhinoLocalHide(map, "toString", function () {
    if (render === null) {
      throw __rhinoLocalAtUnmeasured("String(" + label + ")");
    }
    return render();
  });
  __rhinoLocalAtFaulting(map, label, [
    "containsKey", "containsValue", "entrySet", "keySet", "values", "isEmpty", "put", "putAll", "remove", "clear",
    "forEach", "getOrDefault", "getClass", "hashCode", "equals",
  ]);
  __rhinoLocalAtToJSON(map, label);
  return map;
}

// Header lookups are case-insensitive on AIC (measured: `["content-type"]` and
// `["Content-Type"]` both answer). A JS object cannot be, so each header is
// reachable by its seeded spelling, all lower case, and Title-Case.
function __rhinoLocalAtHeaderAliases(names) {
  var aliases = {};
  var i;
  for (i = 0; i < names.length; i += 1) {
    aliases[names[i].toLowerCase()] = names[i];
    aliases[names[i].toLowerCase().replace(/(^|-)([a-z])/g, function (m, dash, c) {
      return dash + c.toUpperCase();
    })] = names[i];
  }
  return aliases;
}

function __rhinoLocalAtRenderMap(values, legacy) {
  var keys = Object.keys(values);
  var parts = [];
  var i;
  for (i = 0; i < keys.length; i += 1) {
    parts.push(legacy
      ? keys[i] + "=" + __rhinoLocalAtRenderLegacy(values[keys[i]])
      : JSON.stringify(keys[i]) + ": " + __rhinoLocalAtRenderNext(values[keys[i]]));
  }
  return legacy ? "{" + parts.join(", ") + "}" : "{ " + parts.join(", ") + " }";
}

// Seeded values only: AIC holds them as Java strings, lists and Integers.
function __rhinoLocalAtRenderNext(value) {
  if (Array.isArray(value)) {
    var parts = [];
    var i;
    for (i = 0; i < value.length; i += 1) {
      parts.push(__rhinoLocalAtRenderNext(value[i]));
    }
    return "[ " + parts.join(", ") + " ]";
  }
  if (typeof value === "string") {
    return JSON.stringify(value);
  }
  if (typeof value === "number" && value % 1 === 0) {
    return String(value);
  }
  throw __rhinoLocalAtUnmeasured("String(map)", "holding a " + typeof value);
}

function __rhinoLocalAtRenderLegacy(value) {
  if (Array.isArray(value)) {
    return "[" + value.map(__rhinoLocalAtRenderLegacy).join(", ") + "]";
  }
  if (typeof value === "string" || (typeof value === "number" && value % 1 === 0)) {
    return String(value);
  }
  throw __rhinoLocalAtUnmeasured("String(map)", "holding a " + typeof value);
}

function __rhinoLocalAtJsonSafe(value, nested) {
  var i;
  var keys;
  if (value === null) {
    return !nested;
  }
  if (typeof value === "string" || typeof value === "boolean") {
    return true;
  }
  if (typeof value === "number") {
    return isFinite(value);
  }
  if (Array.isArray(value)) {
    for (i = 0; i < value.length; i += 1) {
      if (!__rhinoLocalAtJsonSafe(value[i], true)) {
        return false;
      }
    }
    return true;
  }
  if (__rhinoLocalIsPlainObject(value)) {
    keys = Object.keys(value);
    for (i = 0; i < keys.length; i += 1) {
      if (!__rhinoLocalAtJsonSafe(value[keys[i]], true)) {
        return false;
      }
    }
    return true;
  }
  return false;
}

function __rhinoLocalAtSetField(name, value, method) {
  if (typeof name !== "string") {
    throw __rhinoLocalAtUnmeasured("accessToken." + method + "()", "with a " + typeof name + " field name");
  }
  if (__rhinoLocalAtReserved.indexOf(name) !== -1) {
    throw __rhinoLocalAtUnmeasured("accessToken." + method + "(" + JSON.stringify(name) + ", …)",
      "overwriting a claim AM sets itself");
  }
  if (value === null) {
    // Measured: setField(k, null) leaves the claim absent.
    delete __rhinoLocalAt.state.fields[name];
    return;
  }
  if (!__rhinoLocalAtJsonSafe(value, false)) {
    throw __rhinoLocalAtUnmeasured("accessToken." + method + "()",
      "with a value that is not a string, finite number, boolean, array or plain object (or one holding null)");
  }
  __rhinoLocalAt.state.fields[name] = __rhinoLocalClone(value);
}

function __rhinoLocalAtScopeFromArgument(value) {
  var out = [];
  var i;
  var it;
  var item;
  if (__rhinoLocalAt.engine === "legacy") {
    if (Array.isArray(value)) {
      throw __rhinoLocalInternalError("Cannot convert org.mozilla.javascript.NativeArray@" +
        "rhino-local to java.util.Set");
    }
    if (typeof java !== "undefined" && value instanceof java.util.Set) {
      it = value.iterator();
      while (it.hasNext()) {
        item = it.next();
        if (!(item instanceof java.lang.String)) {
          throw __rhinoLocalAtUnmeasured("accessToken.setScope()", "with a set holding a non-string");
        }
        out.push(String(item));
      }
      return out;
    }
    throw __rhinoLocalAtUnmeasured("accessToken.setScope(" + __rhinoLocalArgShape([value]) + ")");
  }
  var row = __rhinoLocalListRow(value);
  var items = row !== null ? row.items : Array.isArray(value) ? value : null;
  if (items === null) {
    throw __rhinoLocalAtUnmeasured("accessToken.setScope(" + __rhinoLocalArgShape([value]) + ")");
  }
  for (i = 0; i < items.length; i += 1) {
    if (typeof items[i] !== "string") {
      throw __rhinoLocalAtUnmeasured("accessToken.setScope()", "with a non-string scope");
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
  var legacy = __rhinoLocalAt.engine === "legacy";
  var cls = __rhinoLocalAtClass[__rhinoLocalAt.engine];
  var token = {};
  var state = function () {
    return __rhinoLocalAt.state;
  };
  var m = function (name, arity, fn) {
    __rhinoLocalAtMethod(token, cls, name, arity, fn);
  };
  // Getters a script cannot change.
  m("getRealm", 0, function () {
    return __rhinoLocalAtString(seed.realm);
  });
  m("getClientId", 0, function () {
    return __rhinoLocalAtString(seed.clientId);
  });
  m("getResourceOwnerId", 0, function () {
    return __rhinoLocalAtString(seed.resourceOwnerId);
  });
  m("getGrantType", 0, function () {
    return __rhinoLocalAtString(seed.grantType);
  });
  m("getTokenType", 0, function () {
    return __rhinoLocalAtString(seed.tokenType);
  });
  m("getTokenName", 0, function () {
    return __rhinoLocalAtString(seed.tokenName);
  });
  m("getTokenId", 0, function () {
    return __rhinoLocalAtString(seed.tokenId);
  });
  m("getAuditTrackingId", 0, function () {
    return __rhinoLocalAtString(seed.auditTrackingId);
  });
  m("getAuthGrantId", 0, function () {
    return __rhinoLocalAtString(seed.authGrantId);
  });
  m("getAuthTimeSeconds", 0, function () {
    return seed.authTimeSeconds;
  });
  m("getAudience", 0, function () {
    return __rhinoLocalAtList(seed.audience.map(__rhinoLocalAtString));
  });
  m("getExpiryTime", 0, function () {
    return state().expiryTime;
  });
  m("isExpired", 0, function () {
    if (state().expiryTime <= Date.now()) {
      throw __rhinoLocalAtUnmeasured("accessToken.isExpired()", "on a token whose expiry has passed");
    }
    return false;
  });
  m("getScope", 0, function () {
    if (__rhinoLocalAt.scopeRemoved) {
      throw __rhinoLocalAtUnmeasured("accessToken.getScope()", "after removeScopes()");
    }
    if (legacy) {
      // A copy: add() answers true and the token keeps its scope (measured).
      return __rhinoLocalAtJavaSet(state().scope, "accessToken.getScope()", true);
    }
    var list = __rhinoLocalAtList(state().scope.slice());
    // Measured: the next-gen list is immutable, and add() throws "null".
    __rhinoLocalHide(list, "add", function () {
      throw __rhinoLocalInternalError("null");
    });
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
      throw __rhinoLocalAtUnmeasured("accessToken.setExpiryTime(" + __rhinoLocalArgShape(arguments) + ")");
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
    if (!__rhinoLocalIsPlainObject(fields)) {
      throw __rhinoLocalAtUnmeasured("accessToken.setFields(" + __rhinoLocalArgShape(arguments) + ")");
    }
    keys = Object.keys(fields);
    for (i = 0; i < keys.length; i += 1) {
      if (fields[keys[i]] === null) {
        throw __rhinoLocalAtUnmeasured("accessToken.setFields()", "with a null value");
      }
    }
    for (i = 0; i < keys.length; i += 1) {
      __rhinoLocalAtSetField(keys[i], fields[keys[i]], "setFields");
    }
    return undefined;
  });
  m("getField", 1, function (name) {
    if (typeof name !== "string") {
      throw __rhinoLocalAtUnmeasured("accessToken.getField(" + __rhinoLocalArgShape(arguments) + ")");
    }
    var value = Object.prototype.hasOwnProperty.call(state().fields, name) ? __rhinoLocalClone(state().fields[name]) : null;
    return legacy ? __rhinoLocalAtJsonValue(value) : value;
  });
  m("getCustomFields", 0, function () {
    var fields = state().fields;
    var untouched = JSON.stringify(fields) === JSON.stringify(__rhinoLocalAt.initial.fields);
    return __rhinoLocalAtMap("accessToken.getCustomFields()", Object.keys(fields), function () {
      throw __rhinoLocalAtUnmeasured("accessToken.getCustomFields()", "entry values");
    }, untouched ? function () {
      return __rhinoLocalAtRenderMap(fields, legacy);
    } : null);
  });
  m("setNonce", 1, function (value) {
    if (typeof value !== "string") {
      throw __rhinoLocalAtUnmeasured("accessToken.setNonce(" + __rhinoLocalArgShape(arguments) + ")");
    }
    state().nonce = value;
    return undefined;
  });
  m("getNonce", 0, function () {
    return state().nonce === null ? null : __rhinoLocalAtString(state().nonce);
  });
  m("setClaims", 1, function (value) {
    if (typeof value !== "string") {
      throw __rhinoLocalAtUnmeasured("accessToken.setClaims(" + __rhinoLocalArgShape(arguments) + ")");
    }
    state().claims = value;
    return undefined;
  });
  m("getClaims", 0, function () {
    return state().claims === null ? null : __rhinoLocalAtString(state().claims);
  });
  m("setAuthLevel", 1, function (value) {
    if (typeof value !== "number" || value % 1 !== 0) {
      throw __rhinoLocalAtUnmeasured("accessToken.setAuthLevel(" + __rhinoLocalArgShape(arguments) + ")");
    }
    state().authLevel = value;
    return undefined;
  });
  m("getAuthLevel", 0, function () {
    var level = state().authLevel;
    if (level === null) {
      return null;
    }
    return legacy ? __rhinoLocalAtJavaNumber(level) : level;
  });
  m("addExtraData", 2, function (name, value) {
    if (typeof name !== "string" || typeof value !== "string") {
      throw __rhinoLocalAtUnmeasured("accessToken.addExtraData(" + __rhinoLocalArgShape(arguments) + ")");
    }
    state().responseFields[name] = value;
    return undefined;
  });
  ["setAct", "setMayAct", "setPermissions"].forEach(function (name) {
    var key = name === "setAct" ? "act" : name === "setMayAct" ? "mayAct" : "permissions";
    m(name, 1, function (value) {
      if (legacy) {
        throw __rhinoLocalAtNoMethod(cls, name, arguments);
      }
      if (!__rhinoLocalIsPlainObject(value) || !__rhinoLocalAtJsonSafe(value, false)) {
        throw __rhinoLocalAtUnmeasured("accessToken." + name + "(" + __rhinoLocalArgShape(arguments) + ")");
      }
      state()[key] = __rhinoLocalClone(value);
      return undefined;
    });
  });
  m("addExtraJsonData", 2, function (name, value) {
    // Measured on legacy with two strings only (2026-11-10): no overload takes
    // them. docs/api/12's 2026-08-27 list calls it working, without argument
    // types, so any other legacy shape is unmeasured rather than refused.
    if (legacy && typeof name === "string" && typeof value === "string") {
      throw __rhinoLocalAtNoMethod(cls, "addExtraJsonData", arguments);
    }
    if (legacy || typeof name !== "string" || typeof value !== "string") {
      throw __rhinoLocalAtUnmeasured("accessToken.addExtraJsonData(" + __rhinoLocalArgShape(arguments) + ")");
    }
    // Measured: the response body carries the string as given, not parsed.
    state().responseFields[name] = value;
    return undefined;
  });
  ["getAct", "getMayAct", "getPermissions", "getConfirmationKey"].forEach(function (name) {
    var key = name === "getAct" ? "act" : name === "getMayAct" ? "mayAct" : name === "getPermissions" ? "permissions" : null;
    m(name, 0, function () {
      if (key !== null && state()[key] !== null) {
        throw __rhinoLocalAtUnmeasured("accessToken." + name + "()", "after it was set");
      }
      return legacy ? __rhinoLocalAtJsonValue(null) : null;
    });
  });
  m("getResourceOwner", 0, function () {
    if (legacy) {
      throw __rhinoLocalShutterError("org.forgerock.oauth2.core.ResourceOwner");
    }
    throw __rhinoLocalAtUnmeasured("accessToken.getResourceOwner()", "(a ResourceOwnerScriptWrapper on AIC)");
  });
  __rhinoLocalAtFaulting(token, "accessToken", [
    "toMap", "getTokenInfo", "removeAuditTrackingId", "removeAuthGrantId", "removeAuthLevel", "removeAuthTime",
    "removeClaims", "removeClientId", "removeConfirmationKey", "removeGrantType", "removeNonce", "removePermissions",
    "removeRealm", "removeResourceOwnerId", "removeTokenName", "removeTokenType", "setAuditTrackingId",
    "setAuthGrantId", "setAuthTime", "setClientId", "setConfirmationKey", "setGrantType", "setId", "setRealm",
    "setResourceOwnerId", "setTokenName", "setTokenType",
  ].concat(__rhinoLocalAtObjectMethods));
  __rhinoLocalAtToJSON(token, "accessToken");
  return token;
}

function __rhinoLocalAtBuildIdentity(seed) {
  var legacy = __rhinoLocalAt.engine === "legacy";
  // Next-gen: measured 2026-11-10, String(identity) is the wrapper's Java
  // toString (class@hash) while identity.toString() is "Cannot find function
  // toString", so the view has no toString at all and converts through
  // valueOf. A null prototype is what lets Rhino's own missing-member error
  // read as it does on AIC.
  var identity = __rhinoLocalAt.engine === "legacy" ? {} : Object.create(null);
  var attrs = seed.attributes;
  if (legacy) {
    var cls = "com.sun.identity.idm.AMIdentity";
    __rhinoLocalAtMethod(identity, cls, "getName", 0, function () {
      return __rhinoLocalAtJavaString(seed.name);
    });
    __rhinoLocalAtMethod(identity, cls, "getUniversalId", 0, function () {
      return __rhinoLocalAtJavaString(seed.universalId);
    });
    __rhinoLocalAtMethod(identity, cls, "isExists", 0, function () {
      return seed.exists;
    });
    __rhinoLocalAtMethod(identity, cls, "getAttribute", 1, function (name) {
      var key = String(name);
      // Measured 2026-09-10: an attribute the identity lacks is an empty set.
      var values = Object.prototype.hasOwnProperty.call(attrs, key) ? attrs[key] : [];
      return __rhinoLocalAtJavaSet(values, "identity.getAttribute()", false);
    });
    // Measured: identity.exists() is "Cannot call property exists", Rhino's
    // refusal to call a non-function (a bean property of isExists()).
    __rhinoLocalHide(identity, "exists", seed.exists);
    __rhinoLocalHide(identity, "toString", function () {
      return seed.universalId;
    });
    // Never emulate getAttributes(): it returns the whole profile, password
    // included, in clear (.ai/core.md §8).
    __rhinoLocalAtFaulting(identity, "identity", [
      "getAttributes", "setAttributes", "removeAttributes", "getRealm", "getType", "isActive", "store",
      "getMemberships", "isMember", "getAssignedServices", "getClass", "hashCode", "equals",
    ]);
  } else {
    var ngCls = "org.forgerock.openam.scripting.api.identity.ScriptedIdentityScriptWrapper";
    __rhinoLocalAtMethod(identity, ngCls, "getName", 0, function () {
      return seed.name;
    });
    __rhinoLocalAtMethod(identity, ngCls, "getUniversalId", 0, function () {
      return seed.universalId;
    });
    __rhinoLocalAtMethod(identity, ngCls, "exists", 0, function () {
      return seed.exists;
    });
    __rhinoLocalAtMethod(identity, ngCls, "getAttributeValues", 1, function (name) {
      var key = String(name);
      // Measured: an attribute the identity lacks reads as an empty list.
      return __rhinoLocalAtList(Object.prototype.hasOwnProperty.call(attrs, key) ? attrs[key].slice() : []);
    });
    __rhinoLocalHide(identity, "valueOf", function () {
      return ngCls + "@rhino-local";
    });
    __rhinoLocalAtFaulting(identity, "identity", ["setAttribute", "addAttribute", "store", "getClass", "hashCode",
      "equals", "wait", "notify", "notifyAll"]);
  }
  __rhinoLocalAtToJSON(identity, "identity");
  return identity;
}

function __rhinoLocalAtBuildScopes(items) {
  if (__rhinoLocalAt.engine === "legacy") {
    return __rhinoLocalAtJavaSet(items, "scopes", false);
  }
  var store = items.slice();
  var list = __rhinoLocalAtList(store);
  // Measured on next-gen: contains() answers, and add() works on this list
  // (it is not the token's scope; setScope copies it).
  __rhinoLocalHide(list, "contains", function (v) {
    if (arguments.length !== 1) {
      throw __rhinoLocalAtNoMethod("java.util.List", "contains", arguments);
    }
    return store.indexOf(String(v)) !== -1;
  });
  __rhinoLocalHide(list, "add", function (v) {
    if (arguments.length !== 1 || typeof v !== "string") {
      throw __rhinoLocalAtUnmeasured("scopes.add(" + __rhinoLocalArgShape(arguments) + ")");
    }
    store.push(v);
    list[store.length - 1] = v;
    __rhinoLocalHide(list, "length", store.length);
    list[__rhinoLocalListKey].items.push(v);
    return true;
  });
  return list;
}

function __rhinoLocalAtBuildRequest(seed, token) {
  var legacy = __rhinoLocalAt.engine === "legacy";
  var listOf = function (values) {
    return __rhinoLocalAtList(values.map(__rhinoLocalAtString));
  };
  var listMap = function (label, values, aliases) {
    var lists = {};
    Object.keys(values).forEach(function (k) {
      lists[k] = listOf(values[k]);
    });
    return __rhinoLocalAtMap(label, Object.keys(values), function (k) {
      return lists[k];
    }, function () {
      return __rhinoLocalAtRenderMap(values, legacy);
    }, aliases);
  };
  var params = listMap("requestProperties.requestParams", seed.params, null);
  var headers = listMap("requestProperties.requestHeaders", seed.headers,
    __rhinoLocalAtHeaderAliases(Object.keys(seed.headers)));
  var values = {
    realm: __rhinoLocalAtString(token.realm),
    requestHeaders: headers,
    requestParams: params,
    requestUri: __rhinoLocalAtString(seed.uri),
  };
  return __rhinoLocalAtMap("requestProperties", ["realm", "requestHeaders", "requestParams", "requestUri"],
    function (k) {
      return values[k];
    }, null);
}

function __rhinoLocalAtBuildClient(seed) {
  var legacy = __rhinoLocalAt.engine === "legacy";
  var custom = __rhinoLocalAtMap("clientProperties.customProperties", Object.keys(seed.customProperties),
    function () {
      throw __rhinoLocalAtUnmeasured("clientProperties.customProperties", "entry values");
    }, Object.keys(seed.customProperties).length === 0 ? function () {
      return legacy ? "{}" : "{  }";
    } : null);
  var sets = {
    allowedGrantTypes: { values: seed.allowedGrantTypes, javaClass: "java.util.HashSet" },
    allowedScopes: { values: seed.allowedScopes, javaClass: "java.util.TreeSet" },
    allowedResponseTypes: { values: seed.allowedResponseTypes, javaClass: "java.util.LinkedHashSet" },
  };
  var keys = legacy
    ? ["allowedGrantTypes", "clientId", "customProperties", "allowedScopes", "allowedResponseTypes"]
    : ["allowedGrantTypes", "clientId", "customProperties", "redirectUris", "allowedScopes", "allowedResponseTypes"];
  return __rhinoLocalAtMap("clientProperties", keys, function (k) {
    if (k === "clientId") {
      return __rhinoLocalAtString(seed.clientId);
    }
    if (k === "customProperties") {
      return custom;
    }
    if (k === "redirectUris") {
      throw __rhinoLocalAtUnmeasured("clientProperties.redirectUris");
    }
    if (legacy) {
      return __rhinoLocalAtJavaSet(sets[k].values, "clientProperties." + k, false);
    }
    // Measured: next-gen hands over the raw Java set, which the context's
    // class shutter refuses.
    throw __rhinoLocalShutterError(sets[k].javaClass);
  }, null);
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
    fields: at.fields !== undefined ? __rhinoLocalClone(at.fields) : { subname: owner, expires_in: 3599 },
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
    attributes: id.attributes !== undefined ? __rhinoLocalClone(id.attributes) : {},
  };
  var c = given.client || {};
  var client = {
    clientId: c.clientId !== undefined ? c.clientId : clientId,
    allowedGrantTypes: c.allowedGrantTypes !== undefined ? c.allowedGrantTypes.slice() : ["CLIENT_CREDENTIALS"],
    allowedScopes: c.allowedScopes !== undefined ? c.allowedScopes.slice() : scope.slice(),
    allowedResponseTypes: c.allowedResponseTypes !== undefined ? c.allowedResponseTypes.slice() : [],
    customProperties: c.customProperties !== undefined ? __rhinoLocalClone(c.customProperties) : {},
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
    params: r.params !== undefined ? __rhinoLocalClone(r.params) : params,
    headers: r.headers !== undefined ? __rhinoLocalClone(r.headers) : {},
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
  __rhinoLocalAt.engine = given.engine || "next-gen";
  var engine = __rhinoLocalAt.engine;
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
  identity = __rhinoLocalAtBuildIdentity(d.identity);
  scopes = __rhinoLocalAtBuildScopes(d.scopes);
  requestProperties = __rhinoLocalAtBuildRequest(d.request, d.token);
  clientProperties = __rhinoLocalAtBuildClient(d.client);
  if (engine === "legacy") {
    // Measured: bound, and null.
    session = null;
  }
}

function __rhinoLocalHarvestAccessToken() {
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
