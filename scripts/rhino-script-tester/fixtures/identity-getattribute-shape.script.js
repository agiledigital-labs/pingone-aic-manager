// Probe: what does an identity attribute getter actually RETURN?
//
// `identity-attr-mapping` answered "which AM name is populated" and used a
// permissive `sizeOf` that reads `.size()` OR `.length` OR `.toArray().length`.
// That helper agrees with itself on a Set, on a Java array AND on a bare
// string, so it could never distinguish them — the counts it reported are not
// evidence about the container. This fixture is the discriminating half.
//
// It CALLS every candidate member rather than `typeof`-ing it: on a
// Rhino-wrapped Java object `typeof` reports "function" for a method that does
// not exist (verified 2026-08-27, docs/api/12) — typeof is evidence only in the
// negative. Each call is its own try/catch and reports ok/throw.
//
// Discriminators, per probed name:
//   java.util.Set  -> size() ok, toArray() ok, length undefined, charAt throws
//   String[]       -> length is a number, size() throws, charAt throws
//   java.lang.String -> charAt(0) ok, length is the CHARACTER count, size() throws
//
// PRIVACY: emits shapes, call outcomes and counts only — never a value. `mail`
// is PII and is used purely as a populated control, so nothing about it is
// echoed but its element count. `fr-idm-custom-attrs` holds tenant SCHEMA (the
// custom property names), so its top-level keys are safe and are reported.
//
// Safe to delete. Reads only. PROBE_USER is the test account's fr-idm-uuid —
// getIdentity() resolves by managed-object UUID, not userName (see
// identity-resolve-diag).
var PROBE_USER = "838e77f6-0999-4afe-8a3f-13a02b28bf37"; // vuthikTestUsername

function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

// Call `fn` and describe what happened, never what came back.
function attempt(fn) {
  try {
    var v = fn();
    return { ok: true, undef: v === undefined, isNull: v === null };
  } catch (e) {
    // Rhino puts the receiver's toString() into the message — `Cannot find
    // function includes in object [alice@example.com]` — so a raw error string
    // leaks the very value this probe must not echo. Keep the reason, drop the
    // receiver. Learned the hard way on the first run of this fixture.
    return { ok: false, threw: String(e).split(" in object ")[0].slice(0, 120) };
  }
}

// The shape report for one returned container.
function describe(get, safeToEcho) {
  var top = attempt(get);
  if (!top.ok) { return { call: top }; }
  var v = get();
  if (v === null || v === undefined) { return { call: top, value: String(v) }; }

  var r = {
    call: top,
    // `length` is a PROPERTY: read it, do not call it. A Java array has a
    // number here; a java.util.Set has undefined; a string has its char count.
    lengthProp: (typeof v.length),
    size: attempt(function () { return v.size(); }),
    toArray: attempt(function () { return v.toArray(); }),
    contains: attempt(function () { return v.contains("x"); }),
    includes: attempt(function () { return v.includes("x"); }),
    get0: attempt(function () { return v.get(0); }),
    index0: attempt(function () { return v[0]; }),
    // the string tells
    charAt: attempt(function () { return v.charAt(0); }),
    substring: attempt(function () { return v.substring(0, 1); })
  };
  if (typeof v.length === "number") { r.lengthValue = v.length; }
  // Element count, by whichever accessor worked — reported as a NUMBER only.
  if (r.size.ok) { r.count = v.size(); }
  else if (typeof v.length === "number") { r.count = v.length; }
  if (r.toArray.ok) { r.toArrayLen = v.toArray().length; }

  if (safeToEcho) {
    // `[{...}]` (a Set/array of one JSON string) vs `{...}` (a bare string) is
    // itself discriminating, and the custom-attrs bag carries no user data.
    r.asString = String(v).slice(0, 120);
    r.keys = attempt(function () {
      var first = r.toArray.ok ? String(v.toArray()[0])
                               : String(v).replace(/^\[/, "").replace(/\]$/, "");
      var obj = JSON.parse(first);
      var ks = [];
      for (var k in obj) { if (obj.hasOwnProperty(k)) { ks.push(k); } }
      return ks;
    });
    if (r.keys.ok) {
      var first2 = r.toArray.ok ? String(v.toArray()[0])
                                : String(v).replace(/^\[/, "").replace(/\]$/, "");
      var o2 = JSON.parse(first2);
      var ks2 = [];
      for (var k2 in o2) { if (o2.hasOwnProperty(k2)) { ks2.push(k2); } }
      r.customKeys = ks2;
    }
  }
  return r;
}

try {
  var id = idRepository.getIdentity(PROBE_USER);
  var out = {
    // Does the next-gen ScriptedIdentity expose getAttribute at all? The 8.1.1
    // class file has it public alongside getAttributeValues; docs/api/14 only
    // ever recorded getAttributeValues, which is not the same claim.
    getAttribute_customAttrs:
      describe(function () { return id.getAttribute("fr-idm-custom-attrs"); }, true),
    getAttributeValues_customAttrs:
      describe(function () { return id.getAttributeValues("fr-idm-custom-attrs"); }, true),
    // populated single-valued control: proves the shape is a property of the
    // METHOD, not of this one object-valued attribute.
    getAttribute_mail:
      describe(function () { return id.getAttribute("mail"); }, false),
    getAttributeValues_mail:
      describe(function () { return id.getAttributeValues("mail"); }, false),
    // negative control: a name that does not exist. Distinguishes "empty" from
    // "absent" in whatever container we are handed.
    getAttribute_bogus:
      describe(function () { return id.getAttribute("givenNameXYZ"); }, false)
  };
  emit({ ok: true, feature: "identity-getattribute-shape", value: JSON.stringify(out) });
} catch (e) {
  emit({ ok: false, feature: "identity-getattribute-shape", error: String(e) });
}
