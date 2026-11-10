// Access-token modification conformance probe: the setters, and what reading
// a value back gives. Runs unchanged on AIC (both engines) and in the
// emulator. The token AM issues is the other half of the evidence.
var out = {};
function clean(e) { return String(e).split(" in object ")[0].replace(/ \([^()]*#\d+\)$/, ""); }
function tryit(k, fn) { try { var v = fn(); out[k] = v === undefined ? "undefined" : v === null ? "null" : (typeof v) + ":" + String(v); } catch (e) { out[k] = "throw:" + clean(e); } }
tryit("f_str", function () { return accessToken.setField("zz_str", "hello"); });
tryit("f_int", function () { return accessToken.setField("zz_int", 42); });
tryit("f_frac", function () { return accessToken.setField("zz_frac", 1.5); });
tryit("f_bool", function () { return accessToken.setField("zz_bool", true); });
tryit("f_null", function () { return accessToken.setField("zz_null", null); });
tryit("f_obj", function () { return accessToken.setField("zz_obj", { a: 1, s: "x", nested: { deeper: true }, arr: [1, "two"] }); });
tryit("f_arr", function () { return accessToken.setField("zz_arr", ["a", 2, false]); });
tryit("f_over", function () { accessToken.setField("zz_over", "first"); return accessToken.setField("zz_over", "second"); });
tryit("f_unset", function () { accessToken.setField("zz_gone", "x"); return accessToken.setField("zz_gone", null); });
tryit("g_str", function () { var v = accessToken.getField("zz_str"); return Object.prototype.toString.call(v) + " " + typeof v + " " + String(v); });
tryit("g_int", function () { var v = accessToken.getField("zz_int"); return Object.prototype.toString.call(v) + " " + typeof v + " " + String(v); });
tryit("g_obj", function () { var v = accessToken.getField("zz_obj"); return Object.prototype.toString.call(v) + " " + typeof v + " " + String(v); });
tryit("g_objA", function () { var v = accessToken.getField("zz_obj"); return typeof v.a + ":" + String(v.a); });
tryit("g_arr", function () { var v = accessToken.getField("zz_arr"); return Object.prototype.toString.call(v) + " " + typeof v + " " + String(v); });
tryit("g_null", function () { var v = accessToken.getField("zz_null"); return v === null ? "is-null" : String(v); });
tryit("fs", function () { return accessToken.setFields({ zz_fs1: "one", zz_fs2: 2 }); });
tryit("custom_after", function () { var m = accessToken.getCustomFields(); var k = []; for (var x in m) k.push(x); k.sort(); return k.join(","); });
tryit("nonce", function () { return accessToken.setNonce("zz-nonce"); });
tryit("nonce_get", function () { return accessToken.getNonce(); });
tryit("authLevel", function () { return accessToken.setAuthLevel(3); });
tryit("authLevel_get", function () { return accessToken.getAuthLevel(); });
tryit("claims", function () { return accessToken.setClaims("{\"userinfo\":{\"zz\":null}}"); });
tryit("claims_get", function () { return accessToken.getClaims(); });
tryit("extra", function () { return accessToken.addExtraData("zz_extra", "ev"); });
tryit("extraJson", function () { return accessToken.addExtraJsonData("zz_xjson", "{\"k\":1}"); });
tryit("extra_get", function () { return accessToken.getField("zz_extra"); });
tryit("setAct", function () { return accessToken.setAct({ sub: "x" }); });
tryit("setMayAct", function () { return accessToken.setMayAct({ client_id: "x" }); });
tryit("setPermissions", function () { return accessToken.setPermissions({ a: 1 }); });
tryit("setField_one", function () { return accessToken.setField("zz_one"); });
tryit("getField_none", function () { return accessToken.getField(); });
accessToken.setField("aicprobe", JSON.stringify(out));
