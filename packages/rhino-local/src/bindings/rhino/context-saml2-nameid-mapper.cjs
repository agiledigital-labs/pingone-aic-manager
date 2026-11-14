// Overlay for AM's SAML2_NAMEID_MAPPER context (next-gen): the remote
// service provider's NameID mapper. Appended to the shared prelude for a
// case whose `context` names it, after context-common.cjs. Same Rhino
// 1.7.14 restrictions as runtime.cjs: no let, no top-level const, no
// shorthand, no Map/Set/Symbol.
//
// Every behaviour here was measured live on 2026-11-14 against a sandbox
// tenant, by an SP-initiated SSO in which AIC was the IdP
// (docs/api/12-script-bindings-matrix.md, "SAML2 NameID mapper"). Anything
// the measurement did not reach is a named harness fault
// (`saml2-nameid-mapper-unmeasured/<binding>.<member>`), never a guess; see
// docs/rhino-local-gaps.md.
//
// The assertion's NameID text is the script's completion value when that
// value is a string. A completion of undefined fails the SSO with
// "Could not generate NameID". A throw fails the SSO and the exception
// text contains the thrown value. The harvest is the runner's
// completionHarvest, so nothing is appended to the author's source.

var __rhinoLocalNi = {
  format: "",
  remote: "",
  hosted: "",
  mapped: undefined,
  attributes: null,
};

// Globals this context does not bind. `session` is not in this list: the
// measured SSO bound it (typeof "object"). Its members were not read.
var __rhinoLocalNiUnbound = [
  "accessToken", "action", "cacheManager", "callbacks", "claims", "existingSession", "idRepository", "nodeState",
  "outcome", "requestHeaders", "requestParameters", "requestedScopes", "scopeValidatorHelper", "sharedState",
  "transientState", "callbacksBuilder", "requestCookies", "journey", "samlApplication", "oauthApplication",
  "locales", "resumedFromSuspend", "auditEntryDetail", "properties", "config", "availableScopes", "token",
  "scopes", "requestProperties", "clientProperties",
];

var __rhinoLocalNiIdentityClass = "org.forgerock.openam.scripting.api.identity.ScriptedIdentityScriptWrapper";
var __rhinoLocalNiEmailFormat = "urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress";
var __rhinoLocalNiRefusal = "Could not generate NameID";
// Present on the helper in the public script samples. The live enumeration
// did not list them and no probe read one, so a read faults.
var __rhinoLocalNiConstants = [
  "NAMEID_FORMAT_TRANSIENT", "NAMEID_FORMAT_PERSISTENT", "NAMEID_FORMAT_UNSPECIFIED", "NAMEID_FORMAT_EMAIL",
];

__rhinoLocalCtx.prefix = "saml2-nameid-mapper-unmeasured";
__rhinoLocalCtx.noun = "SAML2 NameID mapper";

function __rhinoLocalNiDefaults(given) {
  var nameId = given.nameId || {};
  var identity = given.identity || {};
  var attributes = identity.attributes || {};
  var mail = attributes.mail;
  var mapped = nameId.mappedValue;
  if (mapped === undefined && mail && mail.length === 1 && typeof mail[0] === "string") {
    mapped = mail[0];
  }
  return {
    format: nameId.format !== undefined ? String(nameId.format) : __rhinoLocalNiEmailFormat,
    remote: nameId.remoteEntityId !== undefined ? String(nameId.remoteEntityId) : "http://localhost/sp",
    hosted: nameId.hostedEntityId !== undefined ? String(nameId.hostedEntityId) : "https://idp.example.com",
    mapped: mapped,
    name: identity.name !== undefined ? String(identity.name) : "subject",
    exists: identity.exists !== undefined ? identity.exists === true : true,
    attributes: attributes,
  };
}

function __rhinoLocalNiHelper() {
  var view = __rhinoLocalCtxBare();
  var i;
  __rhinoLocalCtxMethod(view, "nameIDScriptHelper", "nameIDScriptHelper", "getNameIDValue", 0, function () {
    if (__rhinoLocalNi.format !== __rhinoLocalNiEmailFormat || typeof __rhinoLocalNi.mapped !== "string") {
      throw __rhinoLocalCtxUnmeasured("nameIDScriptHelper.getNameIDValue()",
        "with no mapped value; the measured call returned the user's one mail value");
    }
    return __rhinoLocalNi.mapped;
  });
  __rhinoLocalCtxMethod(view, "nameIDScriptHelper", "nameIDScriptHelper", "getNameIDFromSession", 0, function () {
    return null;
  });
  __rhinoLocalCtxMethod(view, "nameIDScriptHelper", "nameIDScriptHelper", "shouldPersistNameIDFormat", 0, function () {
    return false;
  });
  __rhinoLocalCtxMethod(view, "nameIDScriptHelper", "nameIDScriptHelper", "createNameIdentifier", 0, function () {
    throw __rhinoLocalCtxUnmeasured("nameIDScriptHelper.createNameIdentifier()",
      "the function is present with arity 0; calling it was not measured");
  });
  for (i = 0; i < __rhinoLocalNiConstants.length; i += 1) {
    (function (name) {
      Object.defineProperty(view, name, {
        enumerable: false,
        configurable: true,
        get: function () {
          throw __rhinoLocalCtxUnmeasured("nameIDScriptHelper." + name);
        },
      });
    })(__rhinoLocalNiConstants[i]);
  }
  __rhinoLocalCtxToJSON(view, "nameIDScriptHelper");
  __rhinoLocalHide(view, "valueOf", function () {
    throw __rhinoLocalCtxUnmeasured("String(nameIDScriptHelper)");
  });
  __rhinoLocalCtxTag(view, null, "nameIDScriptHelper");
  return __rhinoLocalCtxWatch(view, "nameIDScriptHelper");
}

function __rhinoLocalNiIdentity(seed) {
  var view = __rhinoLocalCtxBare();
  __rhinoLocalCtxMethod(view, "identity", __rhinoLocalNiIdentityClass, "getName", 0, function () {
    return seed.name;
  });
  __rhinoLocalCtxMethod(view, "identity", __rhinoLocalNiIdentityClass, "exists", 0, function () {
    return seed.exists;
  });
  __rhinoLocalCtxMethod(view, "identity", __rhinoLocalNiIdentityClass, "getAttributeValues", 1, function (attributeName) {
    var values;
    if (attributeName !== "mail") {
      throw __rhinoLocalCtxUnmeasured("identity.getAttributeValues()",
        "only getAttributeValues(\"mail\") of one string was measured");
    }
    values = seed.attributes.mail;
    if (!values || values.length !== 1 || typeof values[0] !== "string") {
      throw __rhinoLocalCtxUnmeasured("identity.getAttributeValues()",
        "the measured call returned one mail string");
    }
    var result = __rhinoLocalCtxBare();
    result.length = 1;
    result[0] = values[0];
    return __rhinoLocalNiView(result, 'identity.getAttributeValues("mail")');
  });
  __rhinoLocalCtxFaulting(view, "identity", ["store", "setAttribute", "addAttribute", "getUniversalId"]);
  __rhinoLocalCtxToJSON(view, "identity");
  __rhinoLocalHide(view, "valueOf", function () {
    throw __rhinoLocalCtxUnmeasured("String(identity)");
  });
  __rhinoLocalCtxTag(view, null, "identity");
  return __rhinoLocalCtxWatch(view, "identity");
}

// A plain JS object has no hook for an unknown property read. The trusted
// bridge exposes only these measured properties and faults on every other read
// or write; it never hands out Java list operations the probe did not call.
function __rhinoLocalNiView(target, label) {
  if (!__rhinoLocalCtxHostOp) {
    throw __rhinoLocalCtxUnmeasured(label, "without the JVM view bridge");
  }
  return __rhinoLocalCtxHostOp("measuredView", target, function (member) {
    throw __rhinoLocalCtxUnmeasured(label + "." + member);
  });
}

function __rhinoLocalNiSession() {
  return __rhinoLocalNiView(__rhinoLocalCtxBare(), "session");
}

function __rhinoLocalSeedNameId(given) {
  var d;
  var loggerName;
  given = given || {};
  __rhinoLocalCtxBegin();
  __rhinoLocalCtxUnbind(__rhinoLocalNiUnbound);
  d = __rhinoLocalNiDefaults(given);
  __rhinoLocalNi.format = d.format;
  __rhinoLocalNi.remote = d.remote;
  __rhinoLocalNi.hosted = d.hosted;
  __rhinoLocalNi.mapped = d.mapped;
  __rhinoLocalNi.attributes = d.attributes;
  if (given.realm === undefined) { realm = "/alpha"; }
  loggerName = "scripts.SAML2_NAMEID_MAPPER." + __rhinoLocal.loggerScriptId + ".(" + scriptName + ")";
  __rhinoLocalHide(logger, "getName", function () {
    return loggerName;
  });
  nameIDScriptHelper = __rhinoLocalNiHelper();
  identity = __rhinoLocalNiIdentity(d);
  nameIDFormat = d.format;
  remoteEntityId = d.remote;
  hostedEntityId = d.hosted;
  session = __rhinoLocalNiSession();
  __rhinoLocalCtxSeal();
}

function __rhinoLocalNiResult(completion) {
  if (completion === undefined) {
    return { accepted: false, value: null, format: __rhinoLocalNi.format, refusal: __rhinoLocalNiRefusal };
  }
  if (typeof completion !== "string" || completion === "") {
    throw __rhinoLocalCtxUnmeasured("result",
      "(the script evaluated to " + __rhinoLocalArgShape([completion]) +
      "; only a nonempty string and undefined were measured)");
  }
  return { accepted: true, value: completion, format: __rhinoLocalNi.format, refusal: null };
}

function __rhinoLocalHarvestNameId(completion) {
  __rhinoLocalCtxAudit();
  var result = __rhinoLocalNiResult(completion);
  var effects = JSON.parse(__rhinoLocalHarvest());
  effects.outcome = null;
  effects.discardedOutcome = null;
  effects.nameId = result;
  return JSON.stringify(effects);
}

var nameIDScriptHelper;
var identity;
var nameIDFormat;
var remoteEntityId;
var hostedEntityId;
var session;
