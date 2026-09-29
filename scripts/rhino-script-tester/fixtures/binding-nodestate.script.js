// Probe: nodeState's writers and readers, one probe each. Safe to delete.
// Records return values (chain identity included), then what get / getObject
// / isDefined see after each write, so merge and remove semantics are visible.
// One payload key per probe; Java identity hashes masked.
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
    return { name: n, ok: false, error: String(e) };
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
var r = [];
r.push(probe("putShared/returns-self", function () {
  return nodeState.putShared("rlShared", "s1") === nodeState;
}));
r.push(probe("putTransient/returns-self", function () {
  return nodeState.putTransient("rlTransient", "t1") === nodeState;
}));
r.push(probe("get/shared", function () {
  return nodeState.get("rlShared");
}));
r.push(probe("get/transient", function () {
  return nodeState.get("rlTransient");
}));
r.push(probe("getObject/shared", function () {
  return nodeState.getObject("rlShared");
}));
r.push(probe("isDefined/shared", function () {
  return nodeState.isDefined("rlShared");
}));
r.push(probe("isDefined/transient", function () {
  return nodeState.isDefined("rlTransient");
}));
r.push(probe("isDefined/absent", function () {
  return nodeState.isDefined("rlAbsent");
}));
r.push(probe("get/absent", function () {
  return nodeState.get("rlAbsent");
}));
r.push(probe("getObject/absent", function () {
  return nodeState.getObject("rlAbsent");
}));
r.push(probe("putShared/object", function () {
  nodeState.putShared("rlObj", { a: 1, nested: { b: "x" }, list: [1, 2] });
  return nodeState.get("rlObj");
}));
r.push(probe("getObject/object", function () {
  return nodeState.getObject("rlObj");
}));
r.push(probe("getObject/object/field", function () {
  return nodeState.getObject("rlObj").nested.b;
}));
r.push(probe("mergeShared/returns-self", function () {
  return nodeState.mergeShared({ rlObj: { c: 2 }, rlMerged: "m" }) === nodeState;
}));
r.push(probe("mergeShared/after/replaced-or-deep", function () {
  return nodeState.get("rlObj");
}));
r.push(probe("mergeShared/after/new-key", function () {
  return nodeState.get("rlMerged");
}));
r.push(probe("mergeTransient/returns-self", function () {
  return nodeState.mergeTransient({ rlTransient: "t2", rlMergedT: "mt" }) === nodeState;
}));
r.push(probe("mergeTransient/after", function () {
  return [nodeState.get("rlTransient"), nodeState.get("rlMergedT")];
}));
r.push(probe("putShared/null", function () {
  nodeState.putShared("rlNull", null);
  return [nodeState.isDefined("rlNull"), nodeState.get("rlNull")];
}));
r.push(probe("shadow/transient-over-shared", function () {
  nodeState.putShared("rlBoth", "shared");
  nodeState.putTransient("rlBoth", "transient");
  return nodeState.get("rlBoth");
}));
r.push(probe("remove/returns-self", function () {
  return nodeState.remove("rlShared") === nodeState;
}));
r.push(probe("remove/after", function () {
  return [nodeState.isDefined("rlShared"), nodeState.get("rlShared")];
}));
r.push(probe("remove/both-buckets", function () {
  nodeState.remove("rlBoth");
  return [nodeState.isDefined("rlBoth"), nodeState.get("rlBoth")];
}));
r.push(probe("remove/absent", function () {
  return nodeState.remove("rlAbsent") === nodeState;
}));
r.push(probe("mergeShared/flat", function () {
  return nodeState.mergeShared({ rlFlat: "f", rlFlatNum: 3 }) === nodeState;
}));
r.push(probe("mergeShared/flat/after", function () {
  return [nodeState.get("rlFlat"), nodeState.get("rlFlatNum")];
}));
r.push(probe("keys", function () {
  var keys = nodeState.keys();
  var names = [];
  var it = keys.iterator();
  while (it.hasNext()) {
    var k = String(it.next());
    if (k.indexOf("rl") === 0) {
      names.push(k);
    }
  }
  return { type: typeof keys, rl: names.sort() };
}));
// After "keys", so these keys do not change its list.
r.push(probe("mergeShared/objectAttributes", function () {
  return nodeState.mergeShared({ objectAttributes: { rlOa: 1 } }) === nodeState;
}));
r.push(probe("mergeShared/objectAttributes/after", function () {
  return nodeState.get("objectAttributes");
}));
r.push(probe("mergeTransient/nested", function () {
  return nodeState.mergeTransient({ xNested: { c: 2 } }) === nodeState;
}));
r.push(probe("mergeTransient/nested/after", function () {
  return nodeState.get("xNested");
}));
r.push(probe("getObject/object/methods", function () {
  var o = nodeState.getObject("rlObj");
  return [typeof o.containsKey, typeof o.size, typeof o.keySet];
}));
emit(keyed("binding-nodestate", r));
action.goTo("ok");
