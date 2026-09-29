// Probe: safe remaining binding members and metadata. Safe to delete.
// emailService.send and idRepository.createUser are intentionally not called.
function describe(v) {
  var r = { type: typeof v, string: String(v) };
  try {
    r.array = Array.isArray(v);
  } catch (e) {}
  try {
    r.keys = Object.keys(v);
  } catch (e) {}
  try {
    r.length = v.length;
  } catch (e) {}
  try {
    r.size = v.size();
  } catch (e) {}
  return r;
}
function probe(n, f) {
  try {
    return { name: n, ok: true, value: describe(f()) };
  } catch (e) {
    return { name: n, ok: false, error: String(e) };
  }
}
var r = [];
r.push(
  probe("logger.getName", function () {
    return logger.getName();
  })
);
r.push(
  probe("logger.isTraceEnabled", function () {
    return logger.isTraceEnabled();
  })
);
r.push(
  probe("logger.isDebugEnabled", function () {
    return logger.isDebugEnabled();
  })
);
r.push(
  probe("logger.isErrorEnabled", function () {
    return logger.isErrorEnabled();
  })
);
r.push(
  probe("logger.isInfoEnabled", function () {
    return logger.isInfoEnabled();
  })
);
r.push(
  probe("logger.isWarnEnabled", function () {
    return logger.isWarnEnabled();
  })
);
r.push(
  probe("logger.trace", function () {
    return logger.trace("probe trace");
  })
);
r.push(
  probe("systemEnv.getProperty-1", function () {
    return systemEnv.getProperty("esv.rl.probe.absent");
  })
);
r.push(
  probe("systemEnv.getProperty-2", function () {
    return systemEnv.getProperty("esv.rl.probe.absent", "default-two");
  })
);
r.push(
  probe("systemEnv.getProperty-3-string", function () {
    return systemEnv.getProperty(
      "esv.rl.probe.absent",
      "default-three",
      "java.lang.Integer"
    );
  })
);
r.push(
  probe("systemEnv.getProperty-3-class", function () {
    return systemEnv.getProperty(
      "esv.rl.probe.absent",
      "default-class",
      java.lang.Integer.class
    );
  })
);
r.push(
  probe("systemEnv.getProperty-3-rhino-class-object", function () {
    return systemEnv.getProperty(
      "esv.rl.probe.absent",
      "default-rhino-class",
      java.lang.Integer
    );
  })
);
r.push(
  probe("secrets.getGenericSecret", function () {
    return secrets.getGenericSecret("rl-probe-absent");
  })
);
r.push(
  probe("secrets.getDecryptionKey", function () {
    return secrets.getDecryptionKey("rl-probe-absent");
  })
);
r.push(
  probe("secrets.getEncryptionKey", function () {
    return secrets.getEncryptionKey("rl-probe-absent");
  })
);
r.push(
  probe("secrets.getSigningKey", function () {
    return secrets.getSigningKey("rl-probe-absent");
  })
);
r.push(
  probe("secrets.getVerificationKey", function () {
    return secrets.getVerificationKey("rl-probe-absent");
  })
);
r.push(
  probe("cacheManager.named", function () {
    return cacheManager.named("rl-probe");
  })
);
r.push(
  probe("cacheManager.exists", function () {
    return cacheManager.exists("rl-probe");
  })
);
r.push(
  probe("journey.name", function () {
    return journey.name();
  })
);
r.push(
  probe("journey.innerJourney", function () {
    return journey.innerJourney();
  })
);
r.push(
  probe("journey.mustRun", function () {
    return journey.mustRun();
  })
);
r.push(
  probe("journey.identityResource", function () {
    return journey.identityResource();
  })
);
r.push(
  probe("samlApplication.getApplicationId", function () {
    return samlApplication.getApplicationId();
  })
);
r.push(
  probe("samlApplication.getAuthnRequest", function () {
    return samlApplication.getAuthnRequest();
  })
);
r.push(
  probe("samlApplication.getIdpAttributes", function () {
    return samlApplication.getIdpAttributes();
  })
);
r.push(
  probe("samlApplication.getSpAttributes", function () {
    return samlApplication.getSpAttributes();
  })
);
r.push(
  probe("samlApplication.getFlowInitiator", function () {
    return samlApplication.getFlowInitiator();
  })
);
r.push(
  probe("samlApplication.getAssertion", function () {
    return samlApplication.getAssertion();
  })
);
r.push(
  probe("oauthApplication.getRequestProperties", function () {
    return oauthApplication.getRequestProperties();
  })
);
r.push(
  probe("oauthApplication.getApplicationId", function () {
    return oauthApplication.getApplicationId();
  })
);
r.push(
  probe("oauthApplication.getClientProperties", function () {
    return oauthApplication.getClientProperties();
  })
);
r.push(
  probe("jwtAssertion.generateJwt", function () {
    return jwtAssertion.generateJwt(null);
  })
);
r.push(
  probe("jwtValidator.validateJwtClaims", function () {
    return jwtValidator.validateJwtClaims("invalid");
  })
);
r.push(
  probe("policy.evaluate", function () {
    return policy.evaluate({}, "rl-probe-missing", [], {});
  })
);
r.push(
  probe("policy.evaluateTree", function () {
    return policy.evaluateTree({}, "rl-probe-missing", "/", {});
  })
);
r.push(
  probe("idRepository.getIdentity", function () {
    return idRepository.getIdentity("rl-probe-absent");
  })
);
r.push({
  name: "samlApplication/typeof-enumeration",
  ok: true,
  value: {
    type: typeof samlApplication,
    keys: (function () {
      try {
        return Object.keys(samlApplication);
      } catch (e) {
        return String(e);
      }
    })(),
  },
});
r.push({
  name: "oauthApplication/typeof-enumeration",
  ok: true,
  value: {
    type: typeof oauthApplication,
    keys: (function () {
      try {
        return Object.keys(oauthApplication);
      } catch (e) {
        return String(e);
      }
    })(),
  },
});
r.push({
  name: "emailService/typeof-enumeration",
  ok: true,
  value: {
    type: typeof emailService,
    keys: (function () {
      try {
        return Object.keys(emailService);
      } catch (e) {
        return String(e);
      }
    })(),
  },
});
r.push({ name: "realm", ok: true, value: describe(realm) });
r.push({ name: "scriptName", ok: true, value: describe(scriptName) });
r.push({ name: "cookieName", ok: true, value: describe(cookieName) });
r.push({
  name: "resumedFromSuspend",
  ok: true,
  value: describe(resumedFromSuspend),
});
r.push({ name: "locales", ok: true, value: describe(locales) });
// Round two (2026-09-29): round one showed a class argument is rejected and
// "java.lang.Integer" is "Unsupported return type", so try the other spellings.
var RETURN_TYPES = [
  "string",
  "number",
  "boolean",
  "object",
  "array",
  "list",
  "map",
  "java.lang.String",
  "java.lang.Boolean",
  "java.util.List",
];
for (var t = 0; t < RETURN_TYPES.length; t++) {
  (function (rt) {
    r.push(
      probe("systemEnv.getProperty-3/" + rt, function () {
        return systemEnv.getProperty("esv.rl.probe.absent", "42", rt);
      })
    );
  })(RETURN_TYPES[t]);
}
var JWT_KEY = "0123456789abcdef0123456789abcdef";
var JWT_DATA = {
  jwtType: "SIGNED",
  jwsAlgorithm: "HS256",
  issuer: "https://example.com",
  subject: "probe",
  audience: "https://example.com",
  type: "JWT",
  validityMinutes: 1,
  signingKey: utils.types.stringToBytes(JWT_KEY),
};
var jwt = null;
r.push(
  probe("jwtAssertion.generateJwt/empty", function () {
    return jwtAssertion.generateJwt({});
  })
);
r.push(
  probe("jwtAssertion.generateJwt/HS256", function () {
    jwt = jwtAssertion.generateJwt(JWT_DATA);
    return typeof jwt + "/" + String(jwt).split(".").length + " parts";
  })
);
// "Missing argument" with a byte[] signingKey: retry with the ESV-style string.
r.push(
  probe("jwtAssertion.generateJwt/HS256/string-key", function () {
    var d = {};
    for (var k in JWT_DATA) d[k] = JWT_DATA[k];
    d.signingKey = JWT_KEY;
    jwt = jwtAssertion.generateJwt(d);
    return typeof jwt + "/" + String(jwt).split(".").length + " parts";
  })
);
r.push(
  probe("jwtValidator.validateJwtClaims/empty", function () {
    return JSON.stringify(jwtValidator.validateJwtClaims({}));
  })
);
r.push(
  probe("jwtValidator.validateJwtClaims/HS256", function () {
    return JSON.stringify(
      jwtValidator.validateJwtClaims({
        jwtType: "SIGNED",
        jwt: jwt,
        issuer: "https://example.com",
        subject: "probe",
        audience: "https://example.com",
        type: "JWT",
        verificationKey: utils.types.stringToBytes(JWT_KEY),
      })
    );
  })
);
// { claims } is the subject shape AM accepts (the others are "Invalid value
// subject"); oauth2Scopes is AM's stock policy set (the agent default is absent on AIC).
r.push(
  probe("policy.evaluate/claims/oauth2Scopes", function () {
    return JSON.stringify(
      policy.evaluate(
        { claims: { sub: "rl-probe" } },
        "oauth2Scopes",
        ["https://example.com/"],
        {}
      )
    );
  })
);
r.push(
  probe("policy.evaluateTree/claims/oauth2Scopes", function () {
    return JSON.stringify(
      policy.evaluateTree(
        { claims: { sub: "rl-probe" } },
        "oauth2Scopes",
        "https://example.com/",
        {}
      )
    );
  })
);
var SUBJECTS = [
  { ssoToken: "rl-probe" },
  { jwt: "rl-probe" },
  { claims: { sub: "rl-probe" } },
];
for (var q = 0; q < SUBJECTS.length; q++) {
  (function (subject, label) {
    r.push(
      probe("policy.evaluate/" + label, function () {
        return JSON.stringify(
          policy.evaluate(
            subject,
            "rl-probe-missing",
            ["https://example.com/"],
            {}
          )
        );
      })
    );
  })(SUBJECTS[q], Object.keys(SUBJECTS[q])[0]);
}
if (callbacks.isEmpty())
  callbacksBuilder.hiddenValueCallback("result", JSON.stringify(r));
outcome = "ok";
