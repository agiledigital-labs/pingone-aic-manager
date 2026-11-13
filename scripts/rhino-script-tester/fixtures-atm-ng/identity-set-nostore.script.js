// Next-gen OAUTH2_ACCESS_TOKEN_MODIFICATION, password grant as zzemuR3M-user.
// Calls identity.setAttribute("description", ["zzemuR3M-nostore"]) and does
// not call store(). The managed read after the token request is the
// persistence check. Safe to delete.
var out = { feature: "identity-set-nostore" };
var ATTR = "description";
var VALUE = "zzemuR3M-nostore";

function errText(e) {
  var s = String(e);
  var cut = s.indexOf(" in object ");
  if (cut >= 0) { s = s.slice(0, cut); }
  return s.slice(0, 400);
}

function known(s) {
  return s === "zzemuR3M-nostore" || s === "zzemuR3M-stored" || s === "zzemuR3M-baseline"
    || s === "zzemuR3M-user";
}

function valuesOf(name) {
  try {
    var v = identity.getAttributeValues(name);
    var n = null;
    if (v === null || v === undefined) { return { kind: String(v) }; }
    if (typeof v.length === "number") { n = v.length; }
    else if (typeof v.size === "function") { n = v.size(); }
    var items = [];
    var i;
    if (typeof v.length === "number") {
      for (i = 0; i < v.length && i < 4; i++) {
        var s = String(v[i]);
        items.push(known(s) ? s : "other:len=" + s.length);
      }
    }
    return { kind: "list", n: n, items: items };
  } catch (e) {
    return { kind: "throw", error: errText(e) };
  }
}

try { out.nameIsUser = identity.getName() === "zzemuR3M-user" || identity.getName() == "zzemuR3M-user"; }
catch (e) { out.nameIsUser = "throw:" + errText(e); }
try { out.nameLen = String(identity.getName()).length; } catch (e) { out.nameLen = "throw:" + errText(e); }
try { out.exists = identity.exists(); } catch (e) { out.exists = "throw:" + errText(e); }
out.setType = typeof identity.setAttribute;
out.storeType = typeof identity.store;
out.before = valuesOf(ATTR);
try {
  var ret = identity.setAttribute(ATTR, [VALUE]);
  out.setAttribute = "returned";
  out.retType = typeof ret;
} catch (e) {
  out.setAttribute = "throw";
  out.error = errText(e);
}
out.afterSet = valuesOf(ATTR);
out.storeCalled = false;
try {
  accessToken.setField("zzemuR3Mctl", "ctl-ok");
  out.control = "set";
} catch (e) {
  out.control = "throw:" + errText(e);
}
try { accessToken.setField("aicprobe", out); } catch (e) { /* claim is the only channel */ }
