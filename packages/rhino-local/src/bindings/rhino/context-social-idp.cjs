// Overlay for AM's SOCIAL_IDP_PROFILE_TRANSFORMATION context (legacy and
// next-gen): the social identity provider's `transform` script, which turns
// the provider's raw profile into the normalised one the
// SocialProviderHandlerNodeV2 reads. Appended to the shared prelude for a case
// whose `context` names it, after context-common.cjs. Same Rhino 1.7.14
// restrictions as runtime.cjs: no let, no top-level const, no shorthand, no
// Map/Set/Symbol.
//
// Every behaviour here was measured live on 2026-11-10 against a sandbox
// tenant, by driving a headless social login (an OAuth2 client in one realm as
// the provider, a journey in the other) on both engines
// (docs/api/12-script-bindings-matrix.md, "Social IdP profile
// transformation"). Anything the measurement did not reach is a named harness
// fault (`social-idp-unmeasured/<binding>.<member>`), never a guess; see
// docs/rhino-local-gaps.md.
//
// The legacy context id is shared with the handler node's own `script`
// (which binds `normalizedProfile`); that second invocation is not emulated.
//
// The script's result is the value it evaluates to. The runner hands that
// completion value to __rhinoLocalHarvestSocialIdp (the job's
// `completionHarvest`), so the author's source runs with nothing appended.

var __rhinoLocalSocial = {
  seed: null,
  // Every JsonValue the legacy script constructed, so the harvest can tell
  // one from a lookalike object.
  built: [],
};

// Globals the context does not bind (typeof "undefined" on AIC, measured).
// existingSession is in the next-gen descriptor but is a ReferenceError at
// runtime on both engines (measured).
var __rhinoLocalSocialUnbound = {
  both: [
    "action", "callbacksBuilder", "requestCookies", "existingSession", "cacheManager", "journey", "samlApplication",
    "oauthApplication", "locales", "resumedFromSuspend", "outcome", "idRepository", "accessToken", "identity",
    "session", "claims", "scopes", "normalizedProfile",
  ],
  legacy: [
    "cookieName", "httpClient", "secrets", "openidm", "utils", "emailService", "jwtAssertion", "jwtValidator",
    "policy", "require",
  ],
  "next-gen": ["sharedState", "transientState", "callbacks"],
};

__rhinoLocalListKinds.socialIdp = {
  id: "social-idp-unmeasured/list",
  noun: "a social IdP profile transformation list",
  measured: "only size(), length and String(v) were measured",
  outOfRange: false,
  concatReceiver: false,
  concatArgument: false,
  json: false,
  genericReceiver: false,
};

__rhinoLocalCtx.prefix = "social-idp-unmeasured";
__rhinoLocalCtx.catalog = "social";
__rhinoLocalCtx.noun = "social IdP profile transformation";
__rhinoLocalCtx.listKind = __rhinoLocalListKinds.socialIdp;

// --- rawProfile ----------------------------------------------------------------

// String(rawProfile): every value goes through the same guard as a direct
// read, so rendering cannot admit a value type nobody measured (Sol ctx r3
// P2-2).
function __rhinoLocalSocialRender(profile, keys) {
  var i;
  if (keys.length === 0) {
    throw __rhinoLocalCtxUnmeasured("String(rawProfile)", "of an empty profile");
  }
  for (i = 0; i < keys.length; i += 1) {
    __rhinoLocalSocialEntry(profile, keys[i], keys[i]);
  }
  return __rhinoLocalJavaMapString(profile);
}

// A profile entry read through a member the measurement reached. Only string
// values were measured (the provider's userinfo held nothing else).
function __rhinoLocalSocialEntry(profile, key, member) {
  var value = profile[key];
  if (typeof value !== "string") {
    throw __rhinoLocalCtxUnmeasured("rawProfile." + member, "(a value that is not a string)");
  }
  return value;
}

// Next-gen: a Java map with map access. Measured: `.sub` and get("sub") are
// JS strings, get(missing) is null, `.missing` is undefined, for...in lists
// the keys, keys() is a Java list, size() a number, String(v) is JsonValue's
// rendering and JSON.stringify gives the object.
function __rhinoLocalSocialNextGenProfile(profile) {
  var keys = Object.keys(profile);
  var map = __rhinoLocalCtxMap("rawProfile", keys, function (key) {
    return __rhinoLocalSocialEntry(profile, key, key);
  }, function () {
    return __rhinoLocalSocialRender(profile, keys);
  }, null, {
    get: {
      arity: 1,
      fn: function (key) {
        var k = String(key);
        if (keys.indexOf(k) === -1) {
          return null;
        }
        return __rhinoLocalSocialEntry(profile, k, "get(" + JSON.stringify(k) + ")");
      },
    },
    keys: {
      arity: 0,
      fn: function () {
        var list = __rhinoLocalCtxList(keys.slice(), "rawProfile.keys()", null, "rawProfile.keys");
        // Measured: `[object JavaList]`.
        __rhinoLocalCtxTag(list, "JavaList", "rawProfile.keys()");
        __rhinoLocalCtxRewatch(list);
        return list;
      },
    },
  });
  // Measured: JSON.stringify(rawProfile) is the profile as an object. AIC
  // lists the keys in the Java map's hash order, not insertion order; the
  // view keeps insertion order (docs/rhino-local-gaps.md).
  __rhinoLocalHide(map, "toJSON", function () {
    var out = {};
    var i;
    for (i = 0; i < keys.length; i += 1) {
      Object.defineProperty(out, keys[i], {
        value: __rhinoLocalSocialEntry(profile, keys[i], "JSON.stringify"),
        enumerable: true, configurable: true, writable: true,
      });
    }
    return out;
  });
  __rhinoLocalCtxTag(map, "JavaMap", "rawProfile");
  return map;
}

// A legacy org.forgerock.json.JsonValue holding `value`. Measured on the
// profile's entries: String(v) is the JSON rendering, getObject() of a string
// is a java.lang.String; of the missing entry, String(v) is "null".
function __rhinoLocalSocialJsonValue(value, label) {
  var out = __rhinoLocalCtxJsonValue(value);
  __rhinoLocalCtxMethod(out, label, "org.forgerock.json.JsonValue", "getObject", 0, function () {
    if (typeof value !== "string") {
      throw __rhinoLocalCtxUnmeasured(label + ".getObject()", "of a value that is not a string");
    }
    return __rhinoLocalCtxJavaString(value);
  });
  return __rhinoLocalCtxWatch(out, label);
}

// Legacy: a real JsonValue. Measured: `.sub` is undefined (members only),
// get(k) is a JsonValue, isDefined(k) a boolean, getObject() a Java map that
// prints `{k=v, ...}`, keys() a Java set that prints `[k, ...]`, size() a
// number, String(v) the JSON rendering and JSON.stringify(v) that rendering
// as a JSON string. for...in lists the JsonValue's Java members, which the
// view does not (docs/rhino-local-gaps.md).
function __rhinoLocalSocialLegacyProfile(profile) {
  var keys = Object.keys(profile);
  var out = __rhinoLocalSocialJsonValue(profile, "rawProfile");
  __rhinoLocalHide(out, "toString", function () {
    return __rhinoLocalSocialRender(profile, keys);
  });
  __rhinoLocalCtxMethod(out, "rawProfile", "org.forgerock.json.JsonValue", "get", 1, function (key) {
    var k = String(key);
    var label = "rawProfile.get(" + JSON.stringify(k) + ")";
    return __rhinoLocalSocialJsonValue(
      keys.indexOf(k) === -1 ? null : __rhinoLocalSocialEntry(profile, k, "get(" + JSON.stringify(k) + ")"), label);
  });
  __rhinoLocalCtxMethod(out, "rawProfile", "org.forgerock.json.JsonValue", "isDefined", 1, function (key) {
    return keys.indexOf(String(key)) !== -1;
  });
  __rhinoLocalCtxMethod(out, "rawProfile", "org.forgerock.json.JsonValue", "size", 0, function () {
    return keys.length;
  });
  __rhinoLocalCtxMethod(out, "rawProfile", "org.forgerock.json.JsonValue", "keys", 0, function () {
    var set = __rhinoLocalCtxJavaSet(keys, "rawProfile.keys()", false, undefined, undefined, "rawProfile.keys");
    // Measured: `[object JavaObject]`.
    __rhinoLocalCtxTag(set, "JavaObject", "rawProfile.keys()");
    __rhinoLocalCtxRewatch(set);
    return set;
  });
  __rhinoLocalCtxMethod(out, "rawProfile", "org.forgerock.json.JsonValue", "getObject", 0, function () {
    var map = __rhinoLocalCtxMap("rawProfile.getObject()", keys, function (key) {
      throw __rhinoLocalCtxUnmeasured("rawProfile.getObject()." + key);
    }, function () {
      return "{" + keys.map(function (k) {
        return k + "=" + __rhinoLocalSocialEntry(profile, k, "getObject()");
      }).join(", ") + "}";
    }, null, null);
    __rhinoLocalCtxTag(map, "JavaMap", "rawProfile.getObject()");
    __rhinoLocalCtxRewatch(map);
    return map;
  });
  __rhinoLocalHide(out, "toJSON", function () {
    return String(out);
  });
  return out;
}

// --- org.forgerock.json.JsonValue on the legacy engine -------------------------

// Measured: `new org.forgerock.json.JsonValue(map)` with a java.util
// LinkedHashMap is a result AM accepts; the class's statics (JsonValue.json)
// fail with `Access to Java class "java.lang.Class" is prohibited`.
function __rhinoLocalSocialJsonValueClass(value) {
  if (!(this instanceof __rhinoLocalSocialJsonValueClass)) {
    throw __rhinoLocalCtxUnmeasured("org.forgerock.json.JsonValue(...)", "called without new");
  }
  if (arguments.length !== 1) {
    throw __rhinoLocalCtxUnmeasured("new org.forgerock.json.JsonValue(" + __rhinoLocalArgShape(arguments) + ")");
  }
  __rhinoLocalHide(this, "__rhinoLocalWrapped", value);
  __rhinoLocalCtxFaulting(this, "JsonValue", [
    "get", "getObject", "isNull", "isString", "asString", "isMap", "asMap", "put", "add", "remove", "keys",
    "size", "isDefined", "toString",
  ]);
  __rhinoLocalCtxToJSON(this, "a JsonValue");
  __rhinoLocalSocial.built.push(this);
}

function __rhinoLocalSocialShutteredStatic() {
  throw __rhinoLocalShutterError("java.lang.Class");
}

// --- Defaults and seed -----------------------------------------------------------

function __rhinoLocalSocialDefaults(given) {
  var s = given.socialIdp || {};
  return {
    realm: given.realm !== undefined ? given.realm : "/alpha",
    selectedIdp: s.selectedIdp !== undefined ? s.selectedIdp : "rhino-local-idp",
    rawProfile: s.rawProfile !== undefined ? __rhinoLocalCtxCopy(s.rawProfile) : {},
  };
}

// Runs after __rhinoLocalSeed, on the same seed object.
function __rhinoLocalSeedSocialIdp(given) {
  given = given || {};
  __rhinoLocalCtxBegin(given.engine);
  var engine = __rhinoLocalCtx.engine;
  var legacy = engine === "legacy";
  // Legacy strings are real java.lang.String objects, as JsonValue.getObject()
  // and logger.getName() hand over.
  __rhinoLocalCtx.realJavaStrings = legacy;
  __rhinoLocalCtxUnbind(__rhinoLocalSocialUnbound.both.concat(__rhinoLocalSocialUnbound[engine]));
  var d = __rhinoLocalSocialDefaults(given);
  __rhinoLocalSocial.seed = d;
  __rhinoLocalSocial.built = [];
  if (given.realm === undefined) {
    realm = d.realm;
  }
  selectedIdp = d.selectedIdp;
  // The SelectIdPNode leaves the chosen provider in the node state (measured:
  // nodeState.get("selectedIdp") answers it). Seeded as a starting value, so
  // it is not a write the script made.
  if (!Object.prototype.hasOwnProperty.call(__rhinoLocal.shared, "selectedIdp") &&
    !Object.prototype.hasOwnProperty.call(__rhinoLocal.transient, "selectedIdp")) {
    __rhinoLocal.shared.selectedIdp = d.selectedIdp;
    __rhinoLocal.initialShared.selectedIdp = d.selectedIdp;
  }
  var loggerName = "scripts.SOCIAL_IDP_PROFILE_TRANSFORMATION" + (legacy ? "" : "_NEXT_GEN") + "." +
    __rhinoLocal.loggerScriptId + ".(" + scriptName + ")";
  __rhinoLocalHide(logger, "getName", function () {
    return legacy ? __rhinoLocalCtxJavaString(loggerName) : loggerName;
  });
  if (legacy) {
    rawProfile = __rhinoLocalSocialLegacyProfile(d.rawProfile);
    // Measured: nodeState.get answers a JsonValue on this engine. A key the
    // state does not hold was not read.
    var nodeGet = nodeState.get;
    __rhinoLocalHide(nodeState, "get", function (key) {
      var value = nodeGet.apply(nodeState, arguments);
      if (value === null && !nodeState.isDefined(key)) {
        throw __rhinoLocalCtxUnmeasured("nodeState.get()", "of a key the state does not hold");
      }
      return __rhinoLocalSocialJsonValue(value, "nodeState.get(" + JSON.stringify(String(key)) + ")");
    });
    // Measured: keys() did not list a key the script had just put with
    // putShared; it answered the keys the state held when the script started.
    var startKeys = nodeState.keys();
    __rhinoLocalHide(nodeState, "keys", function () {
      if (arguments.length !== 0) {
        throw __rhinoLocalCtxUnmeasured("nodeState.keys(" + __rhinoLocalArgShape(arguments) + ")");
      }
      return startKeys;
    });
    callbacks = __rhinoLocalCtxBare();
    __rhinoLocalCtxFaulting(callbacks, "callbacks", [
      "isEmpty", "size", "get", "getNameCallbacks", "getPasswordCallbacks", "getTextInputCallbacks",
      "getHiddenValueCallbacks", "getChoiceCallbacks", "getConfirmationCallbacks",
    ]);
    __rhinoLocalCtxToJSON(callbacks, "callbacks");
    __rhinoLocalCtxTag(callbacks, null, "callbacks");
    org.forgerock.json.JsonValue = __rhinoLocalSocialJsonValueClass;
    __rhinoLocalSocialJsonValueClass.json = __rhinoLocalSocialShutteredStatic;
    __rhinoLocalSocialJsonValueClass.object = __rhinoLocalSocialShutteredStatic;
  } else {
    rawProfile = __rhinoLocalSocialNextGenProfile(d.rawProfile);
    // Measured: the context's class shutter refuses the Java set keys() builds.
    __rhinoLocalHide(nodeState, "keys", function () {
      throw __rhinoLocalShutterError("java.util.HashSet");
    });
  }
  __rhinoLocalCtxSeal();
}

// --- Harvest ---------------------------------------------------------------------

function __rhinoLocalSocialIsJava(value) {
  return typeof java !== "undefined" && value !== null && typeof value === "object" &&
    value instanceof java.lang.Object;
}

// One value of a legacy Java map handed to new JsonValue(...).
function __rhinoLocalSocialFromJava(value, path) {
  var out;
  var it;
  var entry;
  if (value instanceof java.lang.String) {
    return String(value);
  }
  if (value instanceof java.lang.Boolean) {
    return value.booleanValue();
  }
  if (value instanceof java.lang.Number) {
    var n = value.doubleValue();
    if (!isFinite(n)) {
      throw __rhinoLocalCtxUnmeasured("result" + path, "(a non-finite number)");
    }
    return n;
  }
  if (value instanceof java.util.Map) {
    out = {};
    it = value.entrySet().iterator();
    while (it.hasNext()) {
      entry = it.next();
      if (!(entry.getKey() instanceof java.lang.String)) {
        throw __rhinoLocalCtxUnmeasured("result" + path, "(a map with a non-string key)");
      }
      Object.defineProperty(out, String(entry.getKey()), {
        value: __rhinoLocalSocialFromJava(entry.getValue(), path + "." + entry.getKey()),
        enumerable: true, configurable: true, writable: true,
      });
    }
    return out;
  }
  if (typeof value === "string" || typeof value === "boolean") {
    return value;
  }
  throw __rhinoLocalCtxUnmeasured("result" + path, "(a value in a JsonValue's map that is not a string, finite number, " +
    "boolean or Java map)");
}

// One value of a plain-object result. Measured at the top level: strings,
// integral and fractional numbers (AM keeps them as doubles), booleans, null,
// undefined (becomes null) and an object holding an array of a number and a
// string. A null or undefined deeper down, and anything not JSON, is unmeasured.
function __rhinoLocalSocialFromJs(value, path, top) {
  var out;
  var keys;
  var i;
  if (value === null || value === undefined) {
    if (!top) {
      throw __rhinoLocalCtxUnmeasured("result" + path, "(a null or undefined nested inside a value)");
    }
    return null;
  }
  if (typeof value === "string" || typeof value === "boolean") {
    return value;
  }
  if (typeof value === "number") {
    if (!isFinite(value)) {
      throw __rhinoLocalCtxUnmeasured("result" + path, "(a non-finite number)");
    }
    return value;
  }
  if (Array.isArray(value)) {
    out = [];
    for (i = 0; i < value.length; i += 1) {
      out.push(__rhinoLocalSocialFromJs(value[i], path + "[" + i + "]", false));
    }
    return out;
  }
  if (__rhinoLocalCtxIsJsonObject(value)) {
    out = {};
    keys = Object.keys(value);
    for (i = 0; i < keys.length; i += 1) {
      Object.defineProperty(out, keys[i], {
        value: __rhinoLocalSocialFromJs(value[keys[i]], path + "." + keys[i], false),
        enumerable: true, configurable: true, writable: true,
      });
    }
    return out;
  }
  throw __rhinoLocalCtxUnmeasured("result" + path, "(" + __rhinoLocalArgShape([value]) +
    "; only strings, finite numbers, booleans, arrays and plain objects were measured)");
}

function __rhinoLocalSocialResult(completion) {
  var refused = function (what) {
    return {
      returned: "none",
      profile: null,
      refusal: "the transformation evaluated to " + what + "; AM fails the journey (401 Login failure)",
    };
  };
  if (completion === undefined) {
    return refused("undefined");
  }
  if (completion === null) {
    return refused("null");
  }
  if (typeof completion === "string") {
    return refused("a string");
  }
  var profile;
  if (__rhinoLocalCtx.engine === "legacy" && __rhinoLocalSocial.built.indexOf(completion) !== -1) {
    var wrapped = completion.__rhinoLocalWrapped;
    if (__rhinoLocalSocialIsJava(wrapped) && wrapped instanceof java.util.Map) {
      profile = __rhinoLocalSocialFromJava(wrapped, "");
    } else {
      throw __rhinoLocalCtxUnmeasured("result", "(a JsonValue built from " + __rhinoLocalArgShape([wrapped]) +
        "; only a java.util.LinkedHashMap was measured)");
    }
  } else if (__rhinoLocalCtxIsJsonObject(completion) && !__rhinoLocalSocialIsJava(completion)) {
    // Measured: a top-level undefined or null property is kept as null.
    profile = {};
    var keys = Object.keys(completion);
    var i;
    for (i = 0; i < keys.length; i += 1) {
      Object.defineProperty(profile, keys[i], {
        value: __rhinoLocalSocialFromJs(completion[keys[i]], "." + keys[i], true),
        enumerable: true, configurable: true, writable: true,
      });
    }
  } else {
    throw __rhinoLocalCtxUnmeasured("result", "(the script evaluated to " + __rhinoLocalArgShape([completion]) +
      "; only undefined, null, a string and a plain object" +
      (__rhinoLocalCtx.engine === "legacy" ? " or new JsonValue(LinkedHashMap)" : "") + " were measured)");
  }
  return { returned: "profile", profile: profile, refusal: null };
}

function __rhinoLocalHarvestSocialIdp(completion) {
  __rhinoLocalCtxAudit();
  var result = __rhinoLocalSocialResult(completion);
  var effects = JSON.parse(__rhinoLocalHarvest());
  // The context decides no outcome; a script that assigns `outcome` assigns
  // an ordinary variable.
  effects.outcome = null;
  effects.discardedOutcome = null;
  effects.socialIdp = result;
  return JSON.stringify(effects);
}

var rawProfile;
var selectedIdp;
