// Overlay for AM's OAUTH2_MAY_ACT_NEXT_GEN context (2.0): the OAuth2 access-token may-act script.
// Appended to the shared prelude for a case whose `context` names it, after
// context-common.cjs. Same Rhino 1.7.14 restrictions as runtime.cjs: no let,
// no top-level const, no shorthand, no Map/Set/Symbol.
//
// Every behaviour here was measured live on 2026-11-10 against a sandbox
// tenant, through client_credentials token requests against a client whose
// accessTokenMayActScript named the script
// (docs/api/12-script-bindings-matrix.md, "OAuth2 may act"). Anything the
// measurement did not reach is a named harness fault
// (`oauth2-may-act-unmeasured/<binding>.<member>`), never a guess; see
// docs/rhino-local-gaps.md.
//
// AM runs the script's body and then, if it did not throw, stamps may_act and
// act from the token. The top-level value is ignored. A thrown script is a
// 500 and discards prior sets. The harvest is appended to the author's source.

var __rhinoLocalMa = {
  slots: null,
  scope: null,
  seedScope: null,
  clientId: "",
  grant: "",
  realm: "",
  expires: 3600,
};

// Globals the context does not bind. `context` stays the engine's
// SimpleScriptContext: reading it wraps that class, which this context's
// shutter refuses (the measured typeof throw).
var __rhinoLocalMaUnbound = [
  "accessToken", "action", "cacheManager", "callbacks", "claims", "existingSession", "idRepository", "nodeState",
  "outcome", "requestHeaders", "requestParameters", "requestedScopes", "scopeValidatorHelper", "sharedState",
  "transientState", "callbacksBuilder", "requestCookies", "journey", "samlApplication", "oauthApplication",
  "locales", "resumedFromSuspend", "auditEntryDetail", "properties", "config", "availableScopes", "session",
];

var __rhinoLocalMaClass = "org.forgerock.openam.oauth2.token.ExchangeableTokenScriptWrapper";
var __rhinoLocalMaGeneric = "Error running may_act script";

// Scopes. Measured: length, size(), [i] a JS string, contains() of a
// string, forEach() of JS strings, toArray() a non-array of JS strings,
// JSON.stringify a JSON array, String(v) Java's "[a]", iterator() the class
// shutter. indexOf, isEmpty, join, slice, concat and for...in were not.
__rhinoLocalListKinds.oauth2MayAct = {
  id: "oauth2-may-act-unmeasured/scopes",
  noun: "a may-act scopes list",
  measured: "only length, size(), [i], String(v), contains(string), forEach(), toArray(), JSON.stringify and " +
    "iterator() (the class shutter) were measured",
  outOfRange: false,
  concatReceiver: false,
  concatArgument: false,
  json: true,
  genericReceiver: false,
  // JSON.stringify of an empty scopes list was not measured (json stays true
  // for a non-empty list). concat was not measured at any length.
  // clientProperties.redirectUris is oauth2MayActRedirect (Sol ctx r10 P2-1):
  // the empty redirect surface does not measure an empty scopes list.
  // emptyRoutes is present and empty so a size-zero scopes list faults as
  // unmeasured, and not with the decision-node request list's sentence.
  emptyRoutes: {},
};

// clientProperties.redirectUris. surface.next-gen.json cpRedirect: the list
// is empty, String "[]" and length 0. JSON was not measured. That grant sits
// on the list BuildClient builds, not on this kind (Sol ctx r11 P2-1).
__rhinoLocalListKinds.oauth2MayActRedirect = {
  id: "oauth2-may-act-unmeasured/clientProperties.redirectUris",
  noun: "a may-act redirect-URI list",
  measured: "only length, size(), [i] and String(v) of an empty list were measured",
  outOfRange: false,
  concatReceiver: false,
  concatArgument: false,
  json: false,
  genericReceiver: false,
  emptyRoutes: {},
};

// scopes.toArray(). The capture records typeof, Array.isArray, length and
// typeof [i] only (element "string"), on a
// one-element list. JSON, borrowed Array methods and index writes were not
// measured (Sol ctx r5 P2-2). emptyRoutes is present and empty so a size-zero
// result faults as unmeasured, and not with the decision-node request list's
// sentence (Sol ctx r12 P2-1). The surface list is not empty.
__rhinoLocalListKinds.oauth2MayActArray = {
  id: "oauth2-may-act-unmeasured/scopes.toArray()",
  noun: "scopes.toArray()",
  measured: "only typeof, Array.isArray, length and typeof [i] were measured",
  outOfRange: false,
  concatReceiver: false,
  concatArgument: false,
  json: false,
  genericReceiver: false,
  emptyRoutes: {},
};

// A requestProperties value. The probe records String, length, [i] and the
// JavaList tag. JSON was not measured; the scope list's json:true must not
// apply here (Sol ctx r5 P2-6). concat and generic receivers were already
// refused by the scope-list kind.
__rhinoLocalListKinds.oauth2MayActRequest = {
  id: "oauth2-may-act-unmeasured/requestProperties",
  noun: "a may-act request value",
  measured: "only length, size(), [i], String(v) and the [object JavaList] tag were measured",
  outOfRange: false,
  concatReceiver: false,
  concatArgument: false,
  json: false,
  genericReceiver: false,
};

__rhinoLocalCtx.prefix = "oauth2-may-act-unmeasured";
__rhinoLocalCtx.catalog = "mayact";
__rhinoLocalCtx.noun = "OAuth2 may act";
__rhinoLocalCtx.listKind = __rhinoLocalListKinds.oauth2MayAct;

function __rhinoLocalMaPlain(value) {
  if (value === null || typeof value !== "object") {return false;}
  if (Object.prototype.toString.call(value) !== "[object Object]") {return false;}
  if (typeof Object.getPrototypeOf === "function" && Object.getPrototypeOf(value) !== Object.prototype) {return false;}
  return true;
}

function __rhinoLocalMaIsJavaMap(value) {
  return typeof java !== "undefined" && value !== null && typeof value === "object" && value instanceof java.util.Map;
}

function __rhinoLocalMaIsJavaList(value) {
  return typeof java !== "undefined" && value !== null && typeof value === "object" && value instanceof java.util.List;
}

function __rhinoLocalMaIsJavaNumber(value) {
  return typeof java !== "undefined" && value !== null && typeof value === "object" && value instanceof java.lang.Number;
}

function __rhinoLocalMaIsJavaString(value) {
  return typeof java !== "undefined" && value !== null && typeof value === "object" && value instanceof java.lang.String;
}

function __rhinoLocalMaIsJavaBoolean(value) {
  return typeof java !== "undefined" && value !== null && typeof value === "object" && value instanceof java.lang.Boolean;
}

function __rhinoLocalMaKind(raw) {
  if (raw === null || raw === undefined) {return "null";}
  if (typeof raw === "string" || __rhinoLocalMaIsJavaString(raw)) {return "string";}
  if (typeof raw === "number" || __rhinoLocalMaIsJavaNumber(raw)) {return "number";}
  if (typeof raw === "boolean") {return "boolean";}
  if (__rhinoLocalMaIsJavaMap(raw)) {return "map";}
  if (__rhinoLocalMaIsJavaList(raw) || Array.isArray(raw)) {return "list";}
  return "other";
}

function __rhinoLocalMaEachItem(raw, fn) {
  var i;
  var n;
  if (__rhinoLocalMaIsJavaList(raw)) {
    n = raw.size();
    for (i = 0; i < n; i += 1) {fn(raw.get(i));}
    return;
  }
  for (i = 0; i < raw.length; i += 1) {fn(raw[i]);}
}

function __rhinoLocalMaEachEntry(raw, fn) {
  var it;
  var key;
  var keys;
  var i;
  if (__rhinoLocalMaIsJavaMap(raw)) {
    it = raw.keySet().iterator();
    while (it.hasNext()) {
      key = it.next();
      fn(String(key), raw.get(key));
    }
    return;
  }
  keys = Object.keys(raw);
  for (i = 0; i < keys.length; i += 1) {fn(keys[i], raw[keys[i]]);}
}

function __rhinoLocalMaSeen(seen, value) {
  var i;
  for (i = 0; i < seen.length; i += 1) {
    if (seen[i] === value) {throw __rhinoLocalCtxUnmeasured("token.setMayAct()", "a cycle");}
  }
  seen.push(value);
}

function __rhinoLocalMaDate(path, claim) {
  if (path.indexOf(".") !== -1 || path.indexOf("[") !== -1 || claim !== "may_act") {
    throw __rhinoLocalCtxUnmeasured("token.setMayAct()", "a Date at " + claim + "." + path);
  }
  var err = { __maDate: path, claim: claim };
  throw err;
}

function __rhinoLocalMaNormalize(value, seen, path, claim) {
  var kind;
  var out;
  var desc;
  if (value === undefined || value === null) {return null;}
  if (typeof value === "boolean" || typeof value === "string") {return value;}
  if (typeof value === "number") {
    if (value !== value) {return "NaN";}
    if (value === Infinity || value === -Infinity) {
      throw __rhinoLocalCtxUnmeasured("token.setMayAct()", "a non-finite number at " + claim + "." + path);
    }
    return value;
  }
  if (typeof value === "function") {
    throw __rhinoLocalCtxUnmeasured("token.setMayAct()", "a function at " + claim + "." + path);
  }
  // A boolean put into a LinkedHashMap comes back as java.lang.Boolean.
  if (__rhinoLocalMaIsJavaBoolean(value)) {return value.booleanValue();}
  if (Object.prototype.toString.call(value) === "[object Date]") {__rhinoLocalMaDate(path, claim);}
  if (__rhinoLocalMaIsJavaNumber(value)) {return __rhinoLocalMaNormalize(Number(String(value)), seen, path, claim);}
  if (__rhinoLocalMaIsJavaString(value)) {return String(value);}
  if (Array.isArray(value) || __rhinoLocalMaIsJavaList(value)) {
    out = [];
    __rhinoLocalMaSeen(seen, value);
    __rhinoLocalMaEachItem(value, function (item) {
      out.push(__rhinoLocalMaNormalize(item, seen, path + "[" + out.length + "]", claim));
    });
    return out;
  }
  kind = __rhinoLocalMaKind(value);
  if (kind === "map" || __rhinoLocalMaPlain(value)) {
    out = {};
    __rhinoLocalMaSeen(seen, value);
    __rhinoLocalMaEachEntry(value, function (key, child) {
      if (__rhinoLocalMaPlain(value)) {
        desc = Object.getOwnPropertyDescriptor(value, key);
        if (desc !== undefined && typeof desc.get === "function") {
          throw __rhinoLocalCtxUnmeasured("token.setMayAct()", "a getter at " + claim + "." + key);
        }
      }
      var childPath = path === "" ? key : path + "." + key;
      out[key] = __rhinoLocalMaNormalize(child, seen, childPath, claim);
    });
    return out;
  }
  throw __rhinoLocalCtxUnmeasured("token.setMayAct()", "a " + __rhinoLocalArgShape([value]) + " at " + claim + "." + path);
}

function __rhinoLocalMaNormalizeRoot(value, claim) {
  return __rhinoLocalMaNormalize(value, [], "", claim);
}

function __rhinoLocalMaDateMessage(key) {
  return "Failed to write json: com.fasterxml.jackson.databind.JsonMappingException: No Context associated with " +
    "current Thread (through reference chain: java.util.LinkedHashMap[\"" + "may_act" + "\"]->org.mozilla.javascript." +
    "NativeObject[\"" + key + "\"]->org.mozilla.javascript.NativeDate[\"prototype\"]->org.mozilla.javascript." +
    "NativeDate[\"allIds\"])";
}

function __rhinoLocalMaReadSlot(which) {
  var slot = __rhinoLocalMa.slots[which];
  if (slot.state === "value") {return slot.value;}
  return null;
}

function __rhinoLocalMaField(key) {
  if (key === null) {
    throw __rhinoLocalInternalError("null");
  }
  if (typeof key === "number") {return null;}
  if (typeof key === "object" && !__rhinoLocalMaIsJavaString(key)) {
    // getField({}) does not throw; it reads as null.
    return null;
  }
  if (typeof key !== "string" && !__rhinoLocalMaIsJavaString(key)) {
    throw __rhinoLocalCtxUnmeasured("token.getField(" + __rhinoLocalArgShape([key]) + ")");
  }
  key = String(key);
  if (key === "scope") {throw __rhinoLocalShutterError("java.util.TreeSet");}
  if (key === "may_act") {return __rhinoLocalMaReadSlot("may");}
  if (key === "act") {return __rhinoLocalMaReadSlot("act");}
  if (key === "client_id" || key === "sub") {return __rhinoLocalMa.clientId;}
  if (key === "grant_type") {return __rhinoLocalMa.grant;}
  if (key === "realm") {return __rhinoLocalMa.realm;}
  if (key === "expires_in") {return __rhinoLocalMa.expires;}
  return null;
}

// Rhino's overload message, via the shared signature. A script object is
// "object". A real Java instance (the captured HashMap) is named by its class
// from Java, not by script-visible getClass(): that reflects java.lang.Class,
// which the class shutter refuses, and the capture says setMayAct(java.util.HashMap).
function __rhinoLocalMaSignature(args) {
  return __rhinoLocalScriptSignature(args);
}

function __rhinoLocalMaNoMethod(cls, name, args) {
  return __rhinoLocalInternalError("Can't find method " + cls + "." + name + "(" + __rhinoLocalMaSignature(args) + ").");
}

// Same arity gate as __rhinoLocalCtxMethod, but a refused count throws
// through __rhinoLocalMaNoMethod. A script object is named "object"
// (capture: setMayAct(object,object)); a Java instance is named by its class.
function __rhinoLocalMaMethod(target, cls, name, arity, fn, measuredRefusal) {
  __rhinoLocalHide(target, name, function () {
    var refused = false;
    var i;
    if (arguments.length !== arity) {
      if (measuredRefusal) {
        for (i = 0; i < measuredRefusal.length; i += 1) {
          if (measuredRefusal[i] === arguments.length) {
            refused = true;
          }
        }
      }
      if (refused) {
        throw __rhinoLocalMaNoMethod(cls, name, arguments);
      }
      throw __rhinoLocalCtxUnmeasured("token." + name + "(" + __rhinoLocalArgShape(arguments) + ")",
        "with " + arguments.length + " argument(s); only " + arity + " was measured");
    }
    return fn.apply(this, arguments);
  });
}

// scopes.toArray(): a list view so JSON, concat and borrowed Array methods
// fault, and an index write is audited. size/get/toString/getClass come with
// every list view and were not measured on this value, so they fault too.
// The tag was not measured. Elements stay JS strings, which is what typeof [i]
// recorded.
function __rhinoLocalMaToArray(items) {
  // scopes.toArray(), the row's toArray child. size/get/toString/getClass
  // stay the every-size stubs below, so the call itself does not fault.
  var list = __rhinoLocalListView(items, __rhinoLocalListKinds.oauth2MayActArray, "scopes", "toArray");
  __rhinoLocalCtxFaulting(list, "scopes.toArray()", ["size", "get", "toString", "getClass"]);
  __rhinoLocalCtxTag(list, null, "scopes.toArray()");
  return __rhinoLocalCtxWatch(list, "scopes.toArray()");
}

function __rhinoLocalMaAssign(which, value, cls, name, args) {
  if (value === null) {
    __rhinoLocalMa.slots[which] = { state: "null" };
    return;
  }
  if (typeof value === "function") {
    throw __rhinoLocalInternalError(
      "Cannot convert function to interface java.util.Map since it contains methods with different names");
  }
  if (Array.isArray(value)) {
    throw __rhinoLocalInternalError("Cannot convert org.mozilla.javascript.NativeArray@00000000 to java.util.Map");
  }
  if (__rhinoLocalMaPlain(value)) {
    __rhinoLocalMa.slots[which] = { state: "value", value: value };
    return;
  }
  // Date and RegExp were not among the measured Can't-find-method shapes.
  if (Object.prototype.toString.call(value) === "[object Date]" ||
      Object.prototype.toString.call(value) === "[object RegExp]") {
    throw __rhinoLocalCtxUnmeasured("token." + name + "(" + __rhinoLocalArgShape(args) + ")");
  }
  throw __rhinoLocalMaNoMethod(cls, name, args);
}

function __rhinoLocalMaScopes(items) {
  var store = items.slice();
  var list = __rhinoLocalListView(store, __rhinoLocalListKinds.oauth2MayAct, "scopes");
  __rhinoLocalCtxTag(list, "JavaList", "scopes");
  __rhinoLocalCtxMethod(list, "scopes", "java.util.List", "contains", 1, function (v) {
    if (typeof v !== "string") {
      throw __rhinoLocalCtxUnmeasured("scopes.contains(" + __rhinoLocalArgShape(arguments) + ")");
    }
    __rhinoLocalEmptyOp(list, "contains");
    return store.indexOf(v) !== -1;
  });
  __rhinoLocalCtxMethod(list, "scopes", "java.util.List", "forEach", 1, function (fn) {
    if (typeof fn !== "function") {
      throw __rhinoLocalCtxUnmeasured("scopes.forEach(" + __rhinoLocalArgShape(arguments) + ")");
    }
    __rhinoLocalEmptyOp(list, "forEach");
    var i;
    for (i = 0; i < store.length; i += 1) {fn(store[i]);}
  });
  __rhinoLocalCtxMethod(list, "scopes", "java.util.List", "toArray", 0, function () {
    return __rhinoLocalMaToArray(store.slice());
  });
  __rhinoLocalCtxMethod(list, "scopes", "java.util.List", "iterator", 0, function () {
    throw __rhinoLocalShutterError("java.util.ArrayList$Itr");
  });
  return __rhinoLocalCtxWatch(list, "scopes");
}

function __rhinoLocalMaToken() {
  var cls = __rhinoLocalMaClass;
  var view = __rhinoLocalCtxBare();
  // Ordinary overload errors only for the counts the captures record.
  // getAct and setAct have no captured wrong count, so any other count
  // faults even when the script catches it (Sol ctx r5 P2-3).
  __rhinoLocalMaMethod(view, cls, "getMayAct", 0, function () {
    return __rhinoLocalMaReadSlot("may");
  }, [1]);
  __rhinoLocalMaMethod(view, cls, "getAct", 0, function () {
    return __rhinoLocalMaReadSlot("act");
  });
  __rhinoLocalMaMethod(view, cls, "getField", 1, function (key) {
    return __rhinoLocalMaField(key);
  }, [0]);
  __rhinoLocalMaMethod(view, cls, "setMayAct", 1, function (value) {
    __rhinoLocalMaAssign("may", value, cls, "setMayAct", arguments);
  }, [0, 2]);
  __rhinoLocalMaMethod(view, cls, "setAct", 1, function (value) {
    __rhinoLocalMaAssign("act", value, cls, "setAct", arguments);
  });
  // No toString of its own: AIC's token.toString() is "Cannot find function
  // toString", while String(token) is the class@hash (length 73). valueOf
  // is how a null-prototype object converts. The eight hex digits keep that
  // length; a live hash would not.
  __rhinoLocalHide(view, "valueOf", function () {
    return cls + "@00000000";
  });
  __rhinoLocalCtxToJSON(view, "token");
  __rhinoLocalCtxTag(view, "JavaObject", "token");
  return __rhinoLocalCtxWatch(view, "token");
}

function __rhinoLocalMaDefaults(given) {
  var tokenSeed = given.accessToken || {};
  var client = given.client || {};
  var identity = given.identity || {};
  var request = given.request || {};
  var realm = given.realm !== undefined ? given.realm : (tokenSeed.realm !== undefined ? tokenSeed.realm : "/alpha");
  var bare = realm.charAt(0) === "/" ? realm.substring(1) : realm;
  var clientId = client.clientId !== undefined ? client.clientId
    : (tokenSeed.clientId !== undefined ? tokenSeed.clientId : "test-client");
  var grant = tokenSeed.grantType !== undefined ? tokenSeed.grantType : "client_credentials";
  var scope = given.scopes !== undefined ? given.scopes.slice()
    : (tokenSeed.scope !== undefined ? tokenSeed.scope.slice() : []);
  var name = identity.name !== undefined ? identity.name : clientId;
  var fields = tokenSeed.fields || {};
  var expires = typeof fields.expires_in === "number" ? fields.expires_in : 3600;
  return {
    realm: realm,
    bare: bare,
    clientId: clientId,
    grant: grant,
    scope: scope,
    expires: expires,
    identity: {
      name: name,
      exists: identity.exists !== undefined ? identity.exists : true,
      universalId: identity.universalId !== undefined ? identity.universalId
        : "id=" + name + ",ou=agentonly,o=" + bare + ",ou=services,ou=am-config",
      attributes: identity.attributes !== undefined ? identity.attributes : {},
    },
    client: {
      clientId: clientId,
      allowedGrantTypes: client.allowedGrantTypes !== undefined ? client.allowedGrantTypes.slice() : ["CLIENT_CREDENTIALS"],
      allowedScopes: client.allowedScopes !== undefined ? client.allowedScopes.slice() : scope.slice(),
      allowedResponseTypes: client.allowedResponseTypes !== undefined ? client.allowedResponseTypes.slice() : [],
      customProperties: client.customProperties !== undefined ? client.customProperties : {},
      redirectUris: client.redirectUris !== undefined ? client.redirectUris.slice() : [],
    },
    request: {
      params: request.params !== undefined ? request.params : { client_id: [clientId], grant_type: [grant] },
      headers: request.headers !== undefined ? request.headers : {},
      uri: request.uri !== undefined ? request.uri
        : "https://tenant.example.com/am/oauth2/realms/root/realms/" + bare + "/access_token",
    },
  };
}

function __rhinoLocalSeedMayAct(given) {
  given = given || {};
  __rhinoLocalCtxBegin();
  __rhinoLocalCtxUnbind(__rhinoLocalMaUnbound);
  var d = __rhinoLocalMaDefaults(given);
  __rhinoLocalMa.slots = { may: { state: "absent" }, act: { state: "absent" } };
  __rhinoLocalMa.scope = d.scope.slice();
  __rhinoLocalMa.seedScope = d.scope.slice();
  __rhinoLocalMa.clientId = d.clientId;
  __rhinoLocalMa.grant = d.grant;
  __rhinoLocalMa.realm = d.realm;
  __rhinoLocalMa.expires = d.expires;
  if (given.realm === undefined) {realm = d.realm;}
  var loggerName = "scripts.OAUTH2_MAY_ACT_NEXT_GEN." +
    __rhinoLocal.loggerScriptId + ".(" + scriptName + ")";
  __rhinoLocalHide(logger, "getName", function () {
    return loggerName;
  });
  token = __rhinoLocalMaToken();
  scopes = __rhinoLocalMaScopes(d.scope);
  identity = __rhinoLocalCtxBuildIdentity(d.identity, { tag: "JavaObject" });
  requestProperties = __rhinoLocalCtxBuildRequest(d.request, d.realm, {
    paramsGetMissing: true, javaTags: true, listKind: __rhinoLocalListKinds.oauth2MayActRequest,
  });
  __rhinoLocalCtxTag(requestProperties, "JavaMap", "requestProperties");
  clientProperties = __rhinoLocalCtxBuildClient(d.client, {
    redirectUris: true,
    javaTags: true,
    redirectKind: __rhinoLocalListKinds.oauth2MayActRedirect,
  });
  __rhinoLocalCtxTag(clientProperties, "JavaMap", "clientProperties");
  // `context` is the engine scope's SimpleScriptContext (AmScopes). The scope
  // is not a ScriptableObject, so a getter cannot be installed on it, and
  // reading the binding wraps that class, which this context's shutter
  // refuses. That is the measured `typeof context` throw. Deleting it would
  // make the name unbound instead.
  __rhinoLocalCtxSeal();
}

function __rhinoLocalMaIssued(slot, claim) {
  if (slot.state === "absent") {return { present: false, value: null };}
  if (slot.state === "null") {return { present: true, value: null };}
  return { present: true, value: __rhinoLocalMaNormalizeRoot(slot.value, claim) };
}

function __rhinoLocalMaResult() {
  var may = __rhinoLocalMaIssued(__rhinoLocalMa.slots.may, "may_act");
  var act = __rhinoLocalMaIssued(__rhinoLocalMa.slots.act, "act");
  return {
    status: 200,
    mayActPresent: may.present,
    mayAct: may.value,
    actPresent: act.present,
    act: act.value,
    scope: __rhinoLocalMa.scope.slice(),
    seedScope: __rhinoLocalMa.seedScope.slice(),
    refusal: null,
  };
}

function __rhinoLocalMaFailed(refusal) {
  return {
    status: 500,
    mayActPresent: false,
    mayAct: null,
    actPresent: false,
    act: null,
    scope: null,
    seedScope: __rhinoLocalMa.seedScope.slice(),
    refusal: refusal,
  };
}

function __rhinoLocalHarvestMayAct() {
  var result;
  try {
    result = __rhinoLocalMaResult();
  } catch (e) {
    if (e !== null && typeof e === "object" && typeof e.__maDate === "string" && e.claim === "may_act") {
      result = __rhinoLocalMaFailed(__rhinoLocalMaDateMessage(e.__maDate));
    } else {
      throw e;
    }
  }
  __rhinoLocalCtxAudit();
  var effects = JSON.parse(__rhinoLocalHarvest());
  effects.outcome = null;
  effects.discardedOutcome = null;
  effects.mayAct = result;
  return JSON.stringify(effects);
}

var token;
var scopes;
var identity;
var requestProperties;
var clientProperties;
var session;
