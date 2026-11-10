// Access-token modification conformance probe: scope and expiry. Runs
// unchanged on AIC (both engines) and in the emulator.
var out = {};
var nextGen = typeof require === "function";
function clean(e) { return String(e).split(" in object ")[0].replace(/ \([^()]*#\d+\)$/, "").replace(/NativeArray@[0-9a-z-]+/, "NativeArray@<hash>"); }
function tryit(k, fn) { try { var v = fn(); out[k] = v === undefined ? "undefined" : (typeof v) + ":" + String(v); } catch (e) { out[k] = "throw:" + clean(e); } }
tryit("getAdd", function () { var l = accessToken.getScope(); return l.add("zzemuc-x") + " / " + String(accessToken.getScope()); });
tryit("setArray", function () { return accessToken.setScope(["zzemuc-probe", "zzemuc-added"]); });
if (nextGen) {
  tryit("afterArray", function () { return accessToken.getScope().size(); });
  tryit("bindingAdd", function () { scopes.add("zzemuc-viaBinding"); return String(scopes); });
  tryit("setBinding", function () { return accessToken.setScope(scopes); });
  tryit("bindingAddAfter", function () { scopes.add("zzemuc-later"); return scopes.size() + " / token " + accessToken.getScope().size(); });
} else {
  tryit("setHashSet", function () { var h = new java.util.HashSet(); h.add("zzemuc-probe"); h.add("zzemuc-added"); return accessToken.setScope(h); });
  tryit("afterHashSet", function () { return accessToken.getScope().size(); });
}
tryit("authBase", function () { return accessToken.getExpiryTime() - accessToken.getAuthTimeSeconds() * 1000; });
tryit("setExp", function () { return accessToken.setExpiryTime(accessToken.getAuthTimeSeconds() * 1000 + 600000); });
tryit("expAfter", function () { return accessToken.getExpiryTime() - accessToken.getAuthTimeSeconds() * 1000; });
accessToken.setField("aicprobe", JSON.stringify(out));
