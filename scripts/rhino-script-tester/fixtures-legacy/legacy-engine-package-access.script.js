// Legacy scripted decision probe (evaluatorVersion 1.0): can an author script
// reach the Rhino engine package (org.mozilla.javascript) by name? Safe to delete.
//
// Legacy has no callbacksBuilder, so the result is emitted with the classic
// JavaImporter + Action.send(HiddenValueCallback) path.
var frJava = JavaImporter(
  org.forgerock.openam.auth.node.api.Action,
  com.sun.identity.authentication.callbacks.HiddenValueCallback
);

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

var json = JSON.stringify(results);
if (typeof callbacksBuilder !== "undefined" && callbacks.isEmpty()) {
  callbacksBuilder.hiddenValueCallback("result", json);
  outcome = "ok";
} else if (callbacks.isEmpty()) {
  action = frJava.Action.send(new frJava.HiddenValueCallback("result", json)).build();
} else {
  action = frJava.Action.goTo("ok").build();
}
