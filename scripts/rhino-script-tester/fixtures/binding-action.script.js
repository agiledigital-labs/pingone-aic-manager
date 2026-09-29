// Probe: action methods, chain return shapes, and one invalid type. Safe to delete.
// suspend is skipped because it parks the journey.
// One payload key per probe, so the corpus can record a gap per probe; and
// Java identity hashes (`[B@6d719625`) masked, because they differ every run.
function keyed(feature, records) {
  var out = { ok: true, feature: feature };
  for (var i = 0; i < records.length; i++) {
    var rec = records[i];
    out[rec.name] = rec.ok
      ? { ok: true, value: rec.value }
      : { ok: false, error: rec.error };
  }
  return JSON.stringify(out).replace(/@[0-9a-f]{4,8}\b/g, "@<hash>");
}
function probe(n, f) {
  try {
    var v = f();
    return {
      name: n,
      ok: true,
      type: typeof v,
      string: String(v),
      same: v === action,
    };
  } catch (e) {
    return { name: n, ok: false, error: String(e) };
  }
}
function emit(p) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", p);
  }
  outcome = "ok";
}
var r = [];
r.push(
  probe("withIdentifiedUser/1", function () {
    return action.withIdentifiedUser("probe-withIdentifiedUser-0");
  })
);
r.push(
  probe("withIdentifiedAgent/1", function () {
    return action.withIdentifiedAgent("probe-withIdentifiedAgent-0");
  })
);
r.push(
  probe("withHeader/1", function () {
    return action.withHeader("probe-withHeader-0");
  })
);
r.push(
  probe("withMaxSessionTime/1", function () {
    return action.withMaxSessionTime(120);
  })
);
r.push(
  probe("withMaxIdleTime/1", function () {
    return action.withMaxIdleTime(120);
  })
);
r.push(
  probe("putSessionProperty/2", function () {
    return action.putSessionProperty(
      "probe-putSessionProperty-0",
      "probe-putSessionProperty-1"
    );
  })
);
r.push(
  probe("withDescription/1", function () {
    return action.withDescription("probe-withDescription-0");
  })
);
r.push(
  probe("withStage/1", function () {
    return action.withStage("probe-withStage-0");
  })
);
r.push(
  probe("withErrorMessage/1", function () {
    return action.withErrorMessage("probe-withErrorMessage-0");
  })
);
r.push(
  probe("withLockoutMessage/1", function () {
    return action.withLockoutMessage("probe-withLockoutMessage-0");
  })
);
r.push(
  probe("removeSessionProperty/1", function () {
    return action.removeSessionProperty("probe-removeSessionProperty-0");
  })
);
r.push(
  probe("withMaxSessionTime/bad", function () {
    return action.withMaxSessionTime("x");
  })
);
emit(keyed("binding-action", r));
action.goTo("ok");
