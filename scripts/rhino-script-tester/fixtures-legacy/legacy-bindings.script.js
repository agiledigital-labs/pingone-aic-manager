// Legacy scripted decision probe (evaluatorVersion 1.0). Safe to delete.
//
// Reports which bindings exist on the LEGACY engine via typeof (non-destructive).
// Legacy has no callbacksBuilder, so results are emitted via the classic
// JavaImporter + Action.send(HiddenValueCallback) path; a next-gen fallback is
// kept so the same harness also works if run on the next-gen engine.
var frJava = JavaImporter(
  org.forgerock.openam.auth.node.api.Action,
  com.sun.identity.authentication.callbacks.HiddenValueCallback
);

function emit(payload) {
  var json = JSON.stringify(payload);
  if (typeof callbacksBuilder !== "undefined" && callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", json);
    outcome = "ok";
    return;
  }
  if (callbacks.isEmpty()) {
    action = frJava.Action.send(
      new frJava.HiddenValueCallback("result", json)
    ).build();
  } else {
    action = frJava.Action.goTo("ok").build();
  }
}

try {
  var b = {};
  b.nodeState = typeof nodeState;
  b.sharedState = typeof sharedState;
  b.transientState = typeof transientState;
  b.callbacks = typeof callbacks;
  b.callbacksBuilder = typeof callbacksBuilder;
  b.action = typeof action;
  b.idRepository = typeof idRepository;
  b.openidm = typeof openidm;
  b.httpClient = typeof httpClient;
  b.utils = typeof utils;
  b.requestHeaders = typeof requestHeaders;
  b.requestParameters = typeof requestParameters;
  b.requestCookies = typeof requestCookies;
  b.existingSession = typeof existingSession;
  b.resumedFromSuspend = typeof resumedFromSuspend;
  b.secrets = typeof secrets;
  b.JavaImporter = typeof JavaImporter;
  b.logger = typeof logger;
  b.realm = typeof realm;
  b.systemEnv = typeof systemEnv;
  b.scriptName = typeof scriptName;
  emit({ ok: true, feature: "legacy-bindings", value: JSON.stringify(b) });
} catch (e) {
  emit({ ok: false, feature: "legacy-bindings", error: String(e) });
}
