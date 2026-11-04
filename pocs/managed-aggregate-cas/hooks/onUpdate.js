/*
 * onUpdate for alpha_poc_agg — the managed-aggregate CAS POC.
 *
 * One body serves every experiment: the record's own `mode` field picks the
 * behaviour, so the hook is installed once and each experiment just creates a
 * parent with the mode it needs. ES5 only (Rhino).
 *
 * Modes:
 *   fold      fold `inbox` into `entries`, recompute `derived`, refuse to move
 *             a terminal outcome (E1, E4, E6, E9, E13, E14, E16, E18)
 *   audit     diff oldObject -> object over `data` and `items`, append one
 *             entry per changed leaf to `audit` in memory (E19)
 *   children  derive from the relationship named by `rel` (E10, E11, E12)
 *   selfpatch fold, then openidm.patch this same record once (E15)
 *
 * `trace: true` on the record makes every run create one alpha_poc_sink record
 * of kind "onUpdate". That is a side effect inside onUpdate on purpose: it is
 * the run counter for E1/E6/E17, and E6 is the claim that it is unsafe.
 *
 * Never log or store `context`: it carries the caller's bearer token.
 */
(function () {
  var runId = String(Math.random()).substring(2, 12);
  var id = String(resourceName).split("/").pop();
  var mode = object.mode || "fold";

  logger.info("POC onUpdate run " + runId + " " + id + " mode " + mode);

  function sink(kind, extra) {
    var rec = {
      kind: kind,
      parent: id,
      runId: runId,
      oldRev: oldObject ? String(oldObject._rev) : null,
      at: Date.now(),
    };
    for (var k in extra) {
      if (Object.prototype.hasOwnProperty.call(extra, k)) rec[k] = extra[k];
    }
    openidm.create("managed/alpha_poc_sink", null, rec);
  }

  function fold() {
    var entries = (object.entries || []).slice();
    var inbox = object.inbox || [];
    for (var i = 0; i < inbox.length; i++) {
      if (inbox[i] && !inbox[i].recalculate) entries.push(inbox[i]);
    }
    var count = 0;
    var sum = 0;
    var best = null;
    for (var j = 0; j < entries.length; j++) {
      var v = Number(entries[j].value) || 0;
      count++;
      sum += v;
      if (best === null || v > best.value) best = { id: entries[j].id, value: v };
    }
    var threshold = Number(object.threshold) || 0;
    var outcome = threshold > 0 && count >= threshold ? "closed" : "open";
    var old = oldObject && oldObject.derived;
    if (old && old.outcome === "closed" && (inbox.length > 0 || outcome !== "closed")) {
      throw { code: 409, message: "POC_TERMINAL: outcome is closed" };
    }
    object.entries = entries;
    object.inbox = [];
    object.derived = {
      count: count,
      sum: sum,
      best: best ? best.id : null,
      outcome: outcome,
      runs: ((old && old.runs) || 0) + 1,
      lastRunId: runId,
    };
  }

  // Leaf-level diff, arrays by index, as JSON-pointer paths.
  function diff(path, a, b, out) {
    var aObj = a !== null && typeof a === "object";
    var bObj = b !== null && typeof b === "object";
    if (aObj && bObj && Array.isArray(a) === Array.isArray(b)) {
      var seen = {};
      var k;
      for (k in a) {
        if (Object.prototype.hasOwnProperty.call(a, k)) {
          seen[k] = true;
          diff(path + "/" + k, a[k], b[k], out);
        }
      }
      for (k in b) {
        if (Object.prototype.hasOwnProperty.call(b, k) && !seen[k]) {
          diff(path + "/" + k, undefined, b[k], out);
        }
      }
      return;
    }
    if (JSON.stringify(a) !== JSON.stringify(b)) {
      out.push({ path: path, from: a === undefined ? null : a, to: b === undefined ? null : b });
    }
  }

  function audit() {
    var changes = [];
    var before = oldObject || {};
    diff("/data", before.data, object.data, changes);
    diff("/items", before.items, object.items, changes);
    var trail = (object.audit || []).slice();
    var seq = Number(before.auditSeq) || 0;
    var ops = request && request.patchOperations ? request.patchOperations.length : null;
    for (var i = 0; i < changes.length; i++) {
      seq++;
      changes[i].seq = seq;
      changes[i].runId = runId;
      changes[i].baseRev = before._rev ? String(before._rev) : null;
      changes[i].ops = ops;
      trail.push(changes[i]);
    }
    object.audit = trail;
    object.auditSeq = seq;
  }

  function children() {
    // `rel` picks the pair: "children" (repo.ds-mapped) or "gchildren" (generic).
    var rel = object.rel === "gchildren" ? "gchildren" : "children";
    var res = openidm.query(String(resourceName) + "/" + rel, { _queryFilter: "true" }, [
      "_ref",
      "_refProperties/status",
      "value",
    ]);
    var count = 0;
    var sum = 0;
    var active = 0;
    for (var i = 0; i < res.result.length; i++) {
      count++;
      sum += Number(res.result[i].value) || 0;
      var rp = res.result[i]._refProperties;
      if (rp && rp.status === "active") active++;
    }
    object.derived = { count: count, sum: sum, active: active, lastRunId: runId };
  }

  if (object.trace) {
    sink("onUpdate", {
      inboxLen: (object.inbox || []).length,
      ops: request && request.patchOperations ? JSON.stringify(request.patchOperations) : null,
      oldCount: oldObject && oldObject.derived ? oldObject.derived.count : null,
    });
  }

  if (mode === "fold") {
    fold();
  } else if (mode === "audit") {
    audit();
  } else if (mode === "children") {
    children();
  } else if (mode === "selfpatch") {
    var marker = false;
    var inbox = object.inbox || [];
    for (var m = 0; m < inbox.length; m++) {
      if (inbox[m] && inbox[m].selfpatch) marker = true;
    }
    fold();
    if (marker) {
      openidm.patch(String(resourceName), null, [
        { operation: "replace", field: "/counter", value: 1000 + Number(object.derived.count) },
      ]);
    }
  }
})();
