// Probe: openidm create / update / patch / delete on a throwaway record in the
// managed/alpha_role type (sandbox), then removes it. Safe to delete.
// Return shapes are recorded with _rev and the generated id masked, because
// they differ every run.
function keyed(feature, records) {
  var out = { ok: true, feature: feature };
  for (var i = 0; i < records.length; i++) {
    var rec = records[i];
    var fields = {};
    for (var k in rec) {
      if (k !== "name") {
        fields[k] = rec[k];
      }
    }
    out[rec.name] = fields;
  }
  return JSON.stringify(out).replace(/@[0-9a-f]{4,8}\b/g, "@<hash>");
}
function probe(n, f) {
  try {
    var v = f();
    return { name: n, ok: true, value: describe(v) };
  } catch (e) {
    // A version mismatch names the record's current _rev, a UUID.
    return { name: n, ok: false, error: String(e).replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}(-[0-9]+)?/g, "<uuid>") };
  }
}
function describe(v) {
  if (v === null || v === undefined) {
    return String(v);
  }
  var out = { type: typeof v, string: String(v) };
  try {
    out.json = JSON.stringify(v);
  } catch (e) {
    out.json = "throws: " + String(e);
  }
  return out;
}
function emit(p) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", p);
  }
  outcome = "ok";
}
var PATH = "managed/alpha_role";
var ID = "rl-probe-openidm-writes";
function masked(v) {
  if (v === null || v === undefined) {
    return String(v);
  }
  var out = {};
  for (var k in v) {
    out[k] = k === "_rev" ? "<rev>" : v[k];
  }
  return out;
}
var r = [];
// Leftover from an interrupted run.
try {
  openidm.delete(PATH + "/" + ID, null);
} catch (e) {
  r.push({ name: "cleanup/before", ok: false, error: String(e) });
}
r.push(probe("create/5", function () {
  return masked(openidm.create(PATH, ID, { name: "rl-a", description: "rl-probe" }, null, null));
}));
r.push(probe("create/duplicate", function () {
  return masked(openidm.create(PATH, ID, { name: "rl-a", description: "rl-probe" }, null, null));
}));
r.push(probe("create/3", function () {
  var made = openidm.create(PATH, null, { name: "rl-generated", description: "rl-probe" });
  openidm.delete(PATH + "/" + made._id, null);
  return typeof made._id;
}));
r.push(probe("create/fields", function () {
  var made = openidm.create(PATH, ID + "-fields", { name: "rl-f", description: "rl-probe" }, null, ["name"]);
  openidm.delete(PATH + "/" + ID + "-fields", null);
  return masked(made);
}));
r.push(probe("update/null-rev", function () {
  return masked(openidm.update(PATH + "/" + ID, null, { name: "rl-a2", description: "rl-probe" }));
}));
r.push(probe("update/after", function () {
  return masked(openidm.read(PATH + "/" + ID));
}));
r.push(probe("update/wrong-rev", function () {
  return masked(openidm.update(PATH + "/" + ID, "0", { name: "rl-a3", description: "rl-probe" }));
}));
r.push(probe("patch/replace", function () {
  return masked(openidm.patch(PATH + "/" + ID, null, [{ operation: "replace", field: "/description", value: "rl-9" }]));
}));
r.push(probe("patch/add-unknown-field", function () {
  return masked(openidm.patch(PATH + "/" + ID, null, [{ operation: "add", field: "/rlNotInSchema", value: "x" }]));
}));
r.push(probe("patch/bad-operation", function () {
  return masked(openidm.patch(PATH + "/" + ID, null, [{ operation: "frobnicate", field: "/description", value: "rl-1" }]));
}));
r.push(probe("patch/fields", function () {
  return masked(openidm.patch(PATH + "/" + ID, null, [{ operation: "replace", field: "/description", value: "rl-8" }], null, ["description"]));
}));
r.push(probe("create/4", function () {
  var made = openidm.create(PATH, ID + "-4", { name: "rl-4", description: "rl-probe" }, {});
  return masked(made);
}));
r.push(probe("update/4", function () {
  return masked(openidm.update(PATH + "/" + ID + "-4", null, { name: "rl-4u", description: "rl-probe" }, {}));
}));
r.push(probe("update/5/fields", function () {
  return masked(openidm.update(PATH + "/" + ID + "-4", null, { name: "rl-4v", description: "rl-probe" }, {}, ["name"]));
}));
r.push(probe("patch/4", function () {
  return masked(openidm.patch(PATH + "/" + ID + "-4", null, [{ operation: "replace", field: "/description", value: "rl-5" }], {}));
}));
// Queries run while the -4 and main records exist. A query response is
// recorded as its sorted keys, its scalar fields and the matched ids, because
// the records' _rev values differ every run.
function queried(resp) {
  var out = { keys: Object.keys(resp).sort(), ids: [], recordKeys: [] };
  for (var k in resp) {
    if (k !== "result") {
      out[k] = resp[k];
    }
  }
  for (var i = 0; i < resp.result.length; i++) {
    out.ids.push(resp.result[i]._id);
    out.recordKeys.push(Object.keys(resp.result[i]).sort().join(","));
  }
  out.ids.sort();
  out.recordKeys.sort();
  return out;
}
r.push(probe("query/2/filter", function () {
  return queried(openidm.query(PATH, { _queryFilter: 'name sw "rl-"' }));
}));
r.push(probe("query/3/fields", function () {
  return queried(openidm.query(PATH, { _queryFilter: 'name sw "rl-"' }, ["name"]));
}));
r.push(probe("query/2/none", function () {
  return queried(openidm.query(PATH, { _queryFilter: 'name eq "rl-none"' }));
}));
r.push(probe("query/2/bad-filter", function () {
  return queried(openidm.query(PATH, { _queryFilter: "name eq" }));
}));
r.push(probe("query/2/no-filter", function () {
  return queried(openidm.query(PATH, {}));
}));
r.push(probe("query/2/absent-type", function () {
  return queried(openidm.query("managed/rlNoSuchType", { _queryFilter: "true" }));
}));
r.push(probe("delete/4/fields", function () {
  return masked(openidm.delete(PATH + "/" + ID + "-4", null, {}, ["name"]));
}));
r.push(probe("action/4/validateObject", function () {
  return openidm.action("policy/" + PATH + "/" + ID, "validateObject", { name: "rl-a", description: "rl-probe" }, {});
}));
r.push(probe("action/3/validateObject", function () {
  return openidm.action("policy/" + PATH + "/" + ID, "validateObject", { name: "rl-a", description: "rl-probe" });
}));
r.push(probe("action/5/validateObject", function () {
  return openidm.action("policy/" + PATH + "/" + ID, "validateObject", { name: "rl-a", description: "rl-probe" }, {}, ["result"]);
}));
r.push(probe("action/2/unknown", function () {
  return openidm.action(PATH, "rlNotAnAction");
}));
r.push(probe("delete/3", function () {
  return masked(openidm.delete(PATH + "/" + ID, null, {}));
}));
r.push(probe("delete", function () {
  return masked(openidm.delete(PATH + "/" + ID, null));
}));
r.push(probe("delete/absent", function () {
  return masked(openidm.delete(PATH + "/" + ID, null));
}));
r.push(probe("update/absent", function () {
  return masked(openidm.update(PATH + "/" + ID, null, { name: "rl-a", description: "rl-probe" }));
}));
// update on an absent id may create it; leave nothing behind.
[ID, ID + "-4", ID + "-fields"].forEach(function (id) {
  try {
    openidm.delete(PATH + "/" + id, null);
  } catch (e) {
    // absent, as intended
  }
});
emit(keyed("binding-openidm-writes", r));
action.goTo("ok");
