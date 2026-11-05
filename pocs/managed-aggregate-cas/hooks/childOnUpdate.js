/*
 * onUpdate for alpha_poc_child — E21.
 *
 * When the child has trace: true, create one alpha_poc_sink of kind
 * "child-onUpdate" with the child id, a run id, and a snapshot of which
 * relationship fields were present on object and oldObject. ES5 only.
 * Never log or store `context` (it carries the bearer). The transaction id
 * string is the one field read off it.
 */
(function () {
  if (!object.trace) return;
  var runId = String(Math.random()).substring(2, 12);
  var id = String(resourceName).split("/").pop();

  function txid() {
    try {
      return String(context.transactionId.transactionId.value);
    } catch (e) {
      return null;
    }
  }

  function edgeBits(edge) {
    if (!edge || typeof edge !== "object") return null;
    var rp = edge._refProperties || null;
    var rpKeys = [];
    if (rp) {
      for (var k in rp) {
        if (Object.prototype.hasOwnProperty.call(rp, k)) rpKeys.push(k);
      }
    }
    rpKeys.sort();
    return {
      hasRef: typeof edge._ref === "string",
      refPropKeys: rpKeys,
      status: rp && Object.prototype.hasOwnProperty.call(rp, "status") ? rp.status : null,
    };
  }

  function shapeRel(v) {
    if (v === null || typeof v === "undefined") return { kind: "null" };
    if (Array.isArray(v)) return { kind: "array", n: v.length, first: v.length ? edgeBits(v[0]) : null };
    if (typeof v === "object") return { kind: "object", edge: edgeBits(v) };
    return { kind: typeof v };
  }

  function snap(rec) {
    if (!rec) return null;
    var names = ["children", "gchildren", "parent", "gparent", "peers", "peerOf", "owner", "owned"];
    var rels = {};
    var keys = [];
    var k;
    for (k in rec) {
      if (Object.prototype.hasOwnProperty.call(rec, k)) keys.push(k);
    }
    keys.sort();
    for (var i = 0; i < names.length; i++) {
      if (Object.prototype.hasOwnProperty.call(rec, names[i])) rels[names[i]] = shapeRel(rec[names[i]]);
    }
    return { keys: keys, rels: rels };
  }

  var seen = null;
  try {
    seen = JSON.stringify({ object: snap(object), old: snap(oldObject) });
  } catch (e2) {
    seen = "snap-failed";
  }
  openidm.create("managed/alpha_poc_sink", null, {
    kind: "child-onUpdate",
    parent: id,
    child: id,
    runId: runId,
    rev: object._rev ? String(object._rev) : null,
    oldRev: oldObject && oldObject._rev ? String(oldObject._rev) : null,
    ops: request && request.patchOperations ? JSON.stringify(request.patchOperations) : null,
    note: seen,
    txid: txid(),
    at: Date.now(),
  });
})();
