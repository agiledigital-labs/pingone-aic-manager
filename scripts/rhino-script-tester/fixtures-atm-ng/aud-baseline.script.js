// Next-gen OAUTH2_ACCESS_TOKEN_MODIFICATION. Does not write `aud`.
// Records the aud the token already has, and sets zzemuR3Mctl the same way a
// custom claim is set, so a later aud write can be compared with a setup that
// is known to work. Safe to delete.
var out = { feature: "aud-baseline" };

function errText(e) {
  var s = String(e);
  var cut = s.indexOf(" in object ");
  if (cut >= 0) { s = s.slice(0, cut); }
  return s.slice(0, 400);
}

function labelOf(el, clientId) {
  if (el === "zzemuR3M-aud" || el === "zzemuR3M-a" || el === "zzemuR3M-b") { return String(el); }
  if (el == "zzemuR3M-aud" || el == "zzemuR3M-a" || el == "zzemuR3M-b") { return String(el); }
  if (clientId !== null && (el === clientId || el == clientId)) { return "client-id"; }
  if (typeof el === "string" && el.indexOf("://") >= 0) { return "url"; }
  if (el === null) { return "null"; }
  return typeof el;
}

function classify(v, clientId) {
  if (v === null) { return { kind: "null" }; }
  if (v === undefined) { return { kind: "undefined" }; }
  var r = { typeof: typeof v, tag: Object.prototype.toString.call(v) };
  if (typeof v === "string") {
    r.kind = "string";
    r.len = v.length;
    r.which = labelOf(v, clientId);
    return r;
  }
  var n = null;
  if (typeof v.length === "number") { n = v.length; }
  else if (typeof v.size === "function") {
    try { n = v.size(); } catch (e) { n = null; }
  }
  if (n !== null && typeof v !== "function") {
    r.kind = "list";
    r.len = n;
    var parts = [];
    var i;
    for (i = 0; i < n && i < 4; i++) {
      try { parts.push(labelOf(v[i], clientId)); }
      catch (e) { parts.push("throw:" + errText(e)); }
    }
    r.elements = parts;
    return r;
  }
  r.kind = typeof v;
  return r;
}

var clientId = null;
try { clientId = accessToken.getClientId(); } catch (e) { out.clientIdError = errText(e); }
out.removeFieldType = typeof accessToken.removeField;
try { out.field = classify(accessToken.getField("aud"), clientId); }
catch (e) { out.field = { kind: "throw", error: errText(e) }; }
try { out.audience = classify(accessToken.getAudience(), clientId); }
catch (e) { out.audience = { kind: "throw", error: errText(e) }; }
try {
  accessToken.setField("zzemuR3Mctl", "ctl-ok");
  out.control = "set";
} catch (e) {
  out.control = "throw:" + errText(e);
}
try { accessToken.setField("aicprobe", out); } catch (e) { /* claim is the only channel */ }
