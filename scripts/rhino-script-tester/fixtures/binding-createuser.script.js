// Probe: createUser overloads and duplicate-name error. Creates two users; delete them afterwards.
// Names and passwords are generated per run; mail uses example.com.
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
function describe(v) {
  var r = { type: typeof v, string: String(v) };
  try {
    r.keys = Object.keys(v).sort();
  } catch (e) {}
  try {
    r.length = v.length;
  } catch (e) {}
  return r;
}
function probe(n, f) {
  try {
    var v = f();
    var r = { name: n, ok: true, value: describe(v) };
    try {
      r.id = v.id;
      r.userId = v.userId;
    } catch (e) {}
    return r;
  } catch (e) {
    return { name: n, ok: false, error: String(e) };
  }
}
var stamp = String(new Date().getTime());
var user = "rl-probe-createuser-" + stamp;
var password = "P" + stamp + "!Aa9";
var first = probe("createUser/2", function () {
  return idRepository.createUser(user, password);
});
var second = probe("createUser/3", function () {
  return idRepository.createUser(user + "-map", password, {
    givenName: "Probe",
    sn: "User",
    mail: "probe@example.com",
  });
});
// Measured 2026-09-29: a string attribute value is a ClassCastException before
// the environment check runs, so the values must be arrays to reach it.
var third = probe("createUser/3/arrays", function () {
  return idRepository.createUser(user + "-arr", password, {
    givenName: ["Probe"],
    sn: ["User"],
    mail: ["probe@example.com"],
  });
});
var duplicate = probe("createUser/duplicate", function () {
  return idRepository.createUser(user, password);
});
if (callbacks.isEmpty()) {
  callbacksBuilder.hiddenValueCallback(
    "result",
    keyed("binding-createuser", [first, second, third, duplicate])
  );
}
outcome = "ok";
