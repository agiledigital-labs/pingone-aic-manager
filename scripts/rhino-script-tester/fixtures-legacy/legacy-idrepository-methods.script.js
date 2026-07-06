// Legacy (evaluatorVersion 1.0) probe: enumerate idRepository methods. Safe to
// delete; only reports typeof for documented/expected members.
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
  var repo = {};
  repo.getIdentity = typeof idRepository.getIdentity;
  repo.getAttribute = typeof idRepository.getAttribute;
  repo.setAttribute = typeof idRepository.setAttribute;
  repo.addAttribute = typeof idRepository.addAttribute;

  emit({
    ok: true,
    feature: "legacy-idrepository-methods",
    idRepository: repo,
  });
} catch (e) {
  emit({
    ok: false,
    feature: "legacy-idrepository-methods",
    error: String(e),
  });
}
