// Probe: which bindings exist in a NEXT-GEN scripted decision context.
// Safe to delete. `typeof <absent>` is "undefined" and never throws, so this is
// non-destructive — it reads no data and calls no binding methods.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
}

try {
  var b = {};
  b.require = typeof require;
  b.openidm = typeof openidm;
  b.httpClient = typeof httpClient;
  b.utils = typeof utils;
  b.logger = typeof logger;
  b.idRepository = typeof idRepository;
  b.nodeState = typeof nodeState;
  b.action = typeof action;
  b.callbacks = typeof callbacks;
  b.callbacksBuilder = typeof callbacksBuilder;
  b.requestHeaders = typeof requestHeaders;
  b.requestParameters = typeof requestParameters;
  b.requestCookies = typeof requestCookies;
  b.sharedState = typeof sharedState;
  b.transientState = typeof transientState;
  b.realm = typeof realm;
  b.systemEnv = typeof systemEnv;
  b.scriptName = typeof scriptName;
  b.secrets = typeof secrets;
  b.existingSession = typeof existingSession;
  b.resumedFromSuspend = typeof resumedFromSuspend;
  b.JavaImporter = typeof JavaImporter;
  b.console = typeof console;
  b.process = typeof process;
  b.Buffer = typeof Buffer;
  b.setTimeout = typeof setTimeout;
  emit({ ok: true, feature: "bindings-availability", value: JSON.stringify(b) });
} catch (e) {
  emit({ ok: false, feature: "bindings-availability", error: String(e) });
}
