// Social provider handler node script probe (legacy SOCIAL_IDP_PROFILE_TRANSFORMATION):
// its own surface, plus the transform's record carried in normalizedProfile.aicprobe.
var out = {};
function clean(e) { return String(e).split(" in object ")[0].replace(/ \([^()]*#\d+\)$/, ""); }
function tag(v) { return Object.prototype.toString.call(v); }
function shape(v) {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  var len = "n/a";
  try { len = typeof v.length === "function" ? "function" : String(v.length); } catch (e) { len = "throw"; }
  return typeof v + "|" + tag(v) + "|" + String(v) + "|len=" + len;
}
function call(fn) { try { return shape(fn()); } catch (e) { return "throw:" + clean(e); } }
function keys(fn) { try { var o = fn(); var k = []; for (var x in o) k.push(x); k.sort(); return k.join(","); } catch (e) { return "throw:" + clean(e); } }
var names = ["rawProfile", "normalizedProfile", "selectedIdp", "nodeState", "sharedState", "transientState", "logger",
  "httpClient", "requestParameters", "requestHeaders", "existingSession", "realm", "scriptName", "cookieName",
  "secrets", "openidm", "require", "identity", "session", "idRepository", "JavaImporter", "Packages", "context"];
out.typeof = {};
for (var i = 0; i < names.length; i++) {
  try { out.typeof[names[i]] = eval("typeof " + names[i]); } catch (e) { out.typeof[names[i]] = "throw:" + clean(e); }
}
out.np = call(function () { return typeof normalizedProfile + "|" + tag(normalizedProfile); });
out.npKeys = keys(function () { return normalizedProfile; });
out.npGiven = call(function () { return normalizedProfile.givenName; });
out.npGivenObj = call(function () { return normalizedProfile.givenName.getObject(); });
out.npGivenIsNull = call(function () { return normalizedProfile.givenName.isNull(); });
out.npGetGiven = call(function () { return normalizedProfile.get("givenName"); });
out.npMissing = call(function () { return normalizedProfile.zzNope; });
out.npMissingIsNull = call(function () { return normalizedProfile.get("zzNope").isNull(); });
out.npId = call(function () { return normalizedProfile.id; });
out.npJson = call(function () { return String(normalizedProfile.toString()); });
out.npNumClass = call(function () { return String(normalizedProfile.get("zzNum").getObject().getClass().getName()); });
var transform = null;
try { transform = String(normalizedProfile.get("aicprobe").getObject()); } catch (e) { transform = "throw:" + clean(e); }
var m = new java.util.LinkedHashMap();
m.put("givenName", "zzNodeGiven");
m.put("zzTransform", transform);
m.put("zzNode", JSON.stringify(out));
new org.forgerock.json.JsonValue(m);
