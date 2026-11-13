// Probe: can an author script reach the Rhino engine package
// (org.mozilla.javascript) by name? Next-gen decision node. Safe to delete.
//
// Each step runs in its own try/catch so one refusal does not hide the rest.
// A result is the step's value, or "THROW <name>: <message>".
var results = {};
function t(label, fn) {
  try {
    results[label] = String(fn());
  } catch (e) {
    results[label] = "THROW " + (e && e.name) + ": " + String(e && e.message);
  }
}

var ctx = null;
t("typeof Packages", function () {
  return typeof Packages;
});
t("typeof Packages.org.mozilla.javascript", function () {
  return typeof Packages.org.mozilla.javascript;
});
t("typeof Packages.org.mozilla.javascript.Context", function () {
  return typeof Packages.org.mozilla.javascript.Context;
});
t("typeof org.mozilla.javascript.Context", function () {
  return typeof org.mozilla.javascript.Context;
});
t("new Packages.org.mozilla.javascript.Context()", function () {
  ctx = new Packages.org.mozilla.javascript.Context();
  return "constructed";
});
t("new org.mozilla.javascript.Context()", function () {
  return new org.mozilla.javascript.Context() ? "constructed" : "falsy";
});
t("JavaImporter(Packages.org.mozilla.javascript)", function () {
  var imp = JavaImporter(Packages.org.mozilla.javascript);
  return typeof imp.Context;
});
t("typeof java.lang.Class", function () {
  return typeof java.lang.Class;
});
t("java.lang.Class.forName(Context)", function () {
  return java.lang.Class.forName("org.mozilla.javascript.Context");
});
t("typeof Packages.org.mozilla.javascript.ScriptableObject", function () {
  return typeof Packages.org.mozilla.javascript.ScriptableObject;
});
if (ctx !== null) {
  t("decompileFunction(ordinary function)", function () {
    return JSON.stringify(ctx.decompileFunction(function ordinary(a) { return a; }, 0));
  });
  t("initStandardObjects()", function () {
    return String(ctx.initStandardObjects());
  });
}

if (callbacks.isEmpty()) {
  callbacksBuilder.hiddenValueCallback("result", JSON.stringify(results));
}
outcome = "ok";
