// Probe: what does `AMIdentity.getAttribute(name)` RETURN, in the one context
// family that actually binds a classic AMIdentity?
//
// `oauth2-access-token.d.ts` and `oidc-claims.d.ts` both declared it
// `JavaArray<JavaString>`. Nothing had measured the container: the 2026-08-27
// binding sweep proved the METHOD resolves (`getAttribute("mail")` returned the
// real address) and never asked whether the thing it returned had `.length` or
// `[0]`. The AM 8.1.1 class file says `getAttribute(String) -> java.util.Set`,
// which is on-prem evidence about a different major version — this fixture is
// the AIC measurement.
//
// It CALLS every candidate member rather than `typeof`-ing it, because in this
// exact context `typeof` reports "function" for a Rhino-wrapped Java method
// that does not exist (verified 2026-08-27, docs/api/12).
//
// Discriminators:
//   java.util.Set    -> size() ok, length undefined, [0] throws, charAt throws
//   java.util.List   -> size() ok AND length a number AND [0] ok
//   String[]         -> length a number, size() throws
//   java.lang.String -> charAt(0) ok, length is the CHARACTER count
//
// PRIVACY: shapes, call outcomes and counts only — never a value, and the
// receiver's toString() is stripped out of every error message (Rhino embeds it,
// which is how the sibling journey fixture leaked a test address on its first
// run). A `client_credentials` grant is used deliberately: the identity is the
// throwaway CLIENT's own agent profile, so there is no user data in reach at
// all. The container's wrapper is a property of the method's return class, not
// of whether the attribute happens to be populated.
//
// Reads only — no setAttribute, no store(). Output rides back as a token claim.
// `objectclass` is multi-valued and populated on every LDAP-backed entry, so it
// covers the POPULATED case; without it the probe only ever measured empty
// containers and could not say the wrapper is the same when there is data in it.
// `fr-idm-custom-attrs` is the attribute in question; `givenNameXYZ` is the
// negative control that distinguishes "empty" from "no such name".
// `com.forgerock.openam.oauth2provider.clientType` is a key the OAuth2 client's
// own agent profile really carries, so it covers the POPULATED case on a
// client_credentials grant without going near `userpassword`. The earlier
// candidates (`objectclass`, `uid`) all came back size 0 on an agentonly
// identity, which measured the wrapper only for an empty Set.
var NAMES = ["fr-idm-custom-attrs", "com.forgerock.openam.oauth2provider.clientType",
             "givenNameXYZ"];

// Call `fn` and describe what happened, never what came back.
function attempt(fn) {
  try {
    var v = fn();
    // "ok" alone hid a null return in the first two runs: a call that resolves
    // and hands back nothing looked identical to one that hands back data.
    if (v === null) { return "ok:null"; }
    if (v === undefined) { return "ok:undefined"; }
    return "ok";
  } catch (e) {
    return "throw:" + String(e).split(" in object ")[0].slice(0, 60);
  }
}

function describe(name) {
  var v;
  try {
    v = identity.getAttribute(name);
  } catch (e) {
    return { call: "throw:" + String(e).split(" in object ")[0].slice(0, 80) };
  }
  if (v === null || v === undefined) { return { call: "ok", value: String(v) }; }

  var r = {
    call: "ok",
    // `length` is a PROPERTY: read it, do not call it.
    lengthProp: typeof v.length,
    size: attempt(function () { return v.size(); }),
    toArray: attempt(function () { return v.toArray(); }),
    contains: attempt(function () { return v.contains("x"); }),
    includes: attempt(function () { return v.includes("x"); }),
    get0: attempt(function () { return v.get(0); }),
    index0: attempt(function () { return v[0]; }),
    iterator: attempt(function () { return v.iterator(); }),
    charAt: attempt(function () { return v.charAt(0); }),
    substring: attempt(function () { return v.substring(0, 1); })
  };
  if (typeof v.length === "number") { r.lengthValue = v.length; }
  if (r.size === "ok") { r.count = v.size(); }
  if (r.toArray === "ok") { r.toArrayLen = v.toArray().length; }
  // Bracketed `[…]` is a collection's toString; a bare string has no brackets.
  // Reported as a SHAPE (first and last character) so no value can ride along.
  var s = String(v);
  r.strFirst = s.length > 0 ? s.charAt(0) : "";
  r.strLast = s.length > 0 ? s.charAt(s.length - 1) : "";
  r.strLen = s.length;
  return r;
}

var out = {};
for (var i = 0; i < NAMES.length; i++) {
  try {
    out[NAMES[i]] = describe(NAMES[i]);
  } catch (e) {
    out[NAMES[i]] = { fixtureError: String(e).slice(0, 100) };
  }
}
// `getAttributes()` is declared to return a Map in the 8.1.1 class file while
// oidc-claims.d.ts calls it a JavaArray. Probe the container only — this object
// carries the client's own `userpassword`, so NOTHING about its contents is
// emitted beyond which accessors resolve.
try {
  var all = identity.getAttributes();
  out.getAttributes = {
    call: "ok",
    lengthProp: typeof all.length,
    size: attempt(function () { return all.size(); }),
    keySet: attempt(function () { return all.keySet(); }),
    entrySet: attempt(function () { return all.entrySet(); }),
    getUid: attempt(function () { return all.get("uid"); }),
    // A Map VALUE is a container too, and this one is populated. Same
    // discriminators, no value echoed.
    valueOfClientType: (function () {
      try {
        var mv = all.get("com.forgerock.openam.oauth2provider.clientType");
        if (mv === null || mv === undefined) { return String(mv); }
        return "len=" + (typeof mv.length)
          + ";size=" + attempt(function () { return mv.size(); })
          + ";count=" + (typeof mv.size === "function" ? mv.size() : "?")
          + ";idx0=" + attempt(function () { return mv[0]; })
          + ";charAt=" + attempt(function () { return mv.charAt(0); });
      } catch (e) { return "throw:" + String(e).split(" in object ")[0].slice(0, 60); }
    })(),
    toArray: attempt(function () { return all.toArray(); }),
    containsKey: attempt(function () { return all.containsKey("uid"); })
  };
} catch (e) {
  out.getAttributes = { call: "throw:" + String(e).split(" in object ")[0].slice(0, 80) };
}

accessToken.setField("aicprobe", out);
