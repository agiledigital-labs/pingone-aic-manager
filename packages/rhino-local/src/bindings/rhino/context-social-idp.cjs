// Overlay for AM's SOCIAL_IDP_PROFILE_TRANSFORMATION context (next-gen): the social identity provider's `transform` script, which turns
// the provider's raw profile into the normalised one the
// SocialProviderHandlerNodeV2 reads. Appended to the shared prelude for a case
// whose `context` names it, after context-common.cjs. Same Rhino 1.7.14
// restrictions as runtime.cjs: no let, no top-level const, no shorthand, no
// Map/Set/Symbol.
//
// Every behaviour here was measured live on 2026-11-10 against a sandbox
// tenant, by driving a headless social login (an OAuth2 client in one realm as
// the provider, a journey in the other)
// (docs/api/12-script-bindings-matrix.md, "Social IdP profile
// transformation"). Anything the measurement did not reach is a named harness
// fault (`social-idp-unmeasured/<binding>.<member>`), never a guess; see
// docs/rhino-local-gaps.md.
//
// The context id is shared with the handler node's own `script` (which binds
// `normalizedProfile`); that second invocation is not emulated.
//
// The script's result is the value it evaluates to. The runner hands that
// completion value to __rhinoLocalHarvestSocialIdp (the job's
// `completionHarvest`), so the author's source runs with nothing appended.

var __rhinoLocalSocial = {
  seed: null,
};

// Globals the context does not bind (typeof "undefined" on AIC, measured).
// existingSession is in the descriptor but is a ReferenceError at runtime
// (measured).
var __rhinoLocalSocialUnbound = [
  "action", "callbacksBuilder", "requestCookies", "existingSession", "cacheManager", "journey", "samlApplication",
  "oauthApplication", "locales", "resumedFromSuspend", "outcome", "idRepository", "accessToken", "identity",
  "session", "claims", "scopes", "normalizedProfile", "sharedState", "transientState", "callbacks",
];

__rhinoLocalCtx.prefix = "social-idp-unmeasured";
__rhinoLocalCtx.noun = "social IdP profile transformation";

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
        // Measured: `[object JavaList]`.
        return __rhinoLocalCtxList(keys.slice());
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
  __rhinoLocalCtxBegin();
  __rhinoLocalCtxUnbind(__rhinoLocalSocialUnbound);
  var d = __rhinoLocalSocialDefaults(given);
  __rhinoLocalSocial.seed = d;
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
  var loggerName = "scripts.SOCIAL_IDP_PROFILE_TRANSFORMATION_NEXT_GEN." +
    __rhinoLocal.loggerScriptId + ".(" + scriptName + ")";
  __rhinoLocalHide(logger, "getName", function () {
    return loggerName;
  });
  rawProfile = __rhinoLocalSocialNextGenProfile(d.rawProfile);
  // Measured: the context's class shutter refuses the Java set keys() builds.
  __rhinoLocalHide(nodeState, "keys", function () {
    throw __rhinoLocalShutterError("java.util.HashSet");
  });
  __rhinoLocalCtxSeal();
}

// --- Harvest ---------------------------------------------------------------------

function __rhinoLocalSocialIsJava(value) {
  return typeof java !== "undefined" && value !== null && typeof value === "object" &&
    value instanceof java.lang.Object;
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
  if (__rhinoLocalCtxIsJsonObject(completion) && !__rhinoLocalSocialIsJava(completion)) {
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
      "; only undefined, null, a string and a plain object were measured)");
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
