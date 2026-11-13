// Access-token modification: what a Java-string-returning getter hands
// the script (measured 2026-11-10 on a client_credentials token for the client
// zzemuC3-atm). Inside a function the value is a java.lang.String: typeof
// "object", `===` a literal false, `==` a literal true, and `==` between two
// getter results true. A top-level `var` reads back as a JS string.
var out = {};
function t(k, f) { try { out[k] = f(); } catch (e) { out[k] = "throw:" + String(e).split(" in object ")[0]; } }
var top = identity.getName();
t("topVarTypeof", function () { return typeof top; });
t("topVarStrictLiteral", function () { return top === "zzemuC3-atm"; });
t("inFn", function () {
  var name = identity.getName();
  var again = identity.getName();
  var client = accessToken.getClientId();
  return {
    typeofName: typeof name,
    typeofClientId: typeof client,
    looseLiteral: name == "zzemuC3-atm",
    strictLiteral: name === "zzemuC3-atm",
    looseSelf: name == again,
    strictSelf: name === again,
    looseNameClientId: name == client,
    strictNameClientId: name === client,
    looseNameUniversalId: name == identity.getUniversalId(),
    text: String(name),
    concatTypeof: typeof (name + ""),
    lengthTypeof: typeof name.length,
    tag: Object.prototype.toString.call(name),
  };
});
t("viaArgTypeof", function () { return (function (v) { return typeof v; })(identity.getName()); });
t("directTypeof", function () { return typeof identity.getName(); });
accessToken.setField("aicprobe", JSON.stringify(out));
