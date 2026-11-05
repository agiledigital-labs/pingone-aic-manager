/*
 * onDelete for alpha_poc_child — E21.
 *
 * When the child has trace: true, create one alpha_poc_sink of kind
 * "child-onDelete" before the child is gone. The sink stores the child id,
 * not a relationship to it. ES5 only. Never log or store `context`.
 */
(function () {
  if (!object.trace) return;
  var runId = String(Math.random()).substring(2, 12);
  var id = String(resourceName).split("/").pop();
  var tx = null;
  try {
    tx = String(context.transactionId.transactionId.value);
  } catch (e) {
    tx = null;
  }
  openidm.create("managed/alpha_poc_sink", null, {
    kind: "child-onDelete",
    parent: id,
    child: id,
    runId: runId,
    rev: object && object._rev ? String(object._rev) : null,
    txid: tx,
    at: Date.now(),
  });
})();
