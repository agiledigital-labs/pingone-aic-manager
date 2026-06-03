// Legacy (evaluatorVersion 1.0) probe: enumerate the actual method surface of
// nodeState and logger via typeof (a Rhino-wrapped Java method reports
// "function"; an absent member reports "undefined"). Non-destructive. Safe to
// delete.
var frJava = JavaImporter(
  org.forgerock.openam.auth.node.api.Action,
  com.sun.identity.authentication.callbacks.HiddenValueCallback
);

function emit(payload) {
  var json = JSON.stringify(payload);
  if (callbacks.isEmpty()) {
    action = frJava.Action.send(
      new frJava.HiddenValueCallback("result", json)
    ).build();
  } else {
    action = frJava.Action.goTo("ok").build();
  }
}

try {
  var ns = {};
  ns.get = typeof nodeState.get;
  ns.getObject = typeof nodeState.getObject;
  ns.putShared = typeof nodeState.putShared;
  ns.putTransient = typeof nodeState.putTransient;
  ns.mergeShared = typeof nodeState.mergeShared;
  ns.mergeTransient = typeof nodeState.mergeTransient;
  ns.sharedState = typeof nodeState.sharedState;
  ns.transientState = typeof nodeState.transientState;
  ns.secureState = typeof nodeState.secureState;
  ns.isDefined = typeof nodeState.isDefined;
  ns.remove = typeof nodeState.remove;

  var lg = {};
  lg.trace = typeof logger.trace;
  lg.debug = typeof logger.debug;
  lg.info = typeof logger.info;
  lg.warn = typeof logger.warn;
  lg.error = typeof logger.error;
  lg.message = typeof logger.message;
  lg.warning = typeof logger.warning;
  lg.messageEnabled = typeof logger.messageEnabled;
  lg.warningEnabled = typeof logger.warningEnabled;
  lg.errorEnabled = typeof logger.errorEnabled;
  lg.isInfoEnabled = typeof logger.isInfoEnabled;

  emit({
    ok: true,
    feature: "legacy-nodestate-logger",
    nodeState: ns,
    logger: lg,
  });
} catch (e) {
  emit({ ok: false, feature: "legacy-nodestate-logger", error: String(e) });
}
