/*
 * postUpdate for alpha_poc_agg — the managed-aggregate CAS POC.
 *
 * The record's `postMode` picks the behaviour:
 *   sink       create one alpha_poc_sink of kind "postUpdate" (E7)
 *   throw      throw after the write (E8)
 *   selfpatch  openidm.patch this same record once, guarded so it cannot loop
 *              (E15)
 *   (absent)   do nothing
 *
 * ES5 only (Rhino). Never log or store `context`.
 */
(function () {
  var id = String(resourceName).split("/").pop();
  var postMode = object.postMode;
  if (!postMode) return;

  if (postMode === "sink") {
    var tx = null;
    try {
      tx = String(context.transactionId.transactionId.value);
    } catch (e) {
      tx = null;
    }
    openidm.create("managed/alpha_poc_sink", null, {
      kind: "postUpdate",
      parent: id,
      rev: object._rev ? String(object._rev) : null,
      oldRev: oldObject && oldObject._rev ? String(oldObject._rev) : null,
      outcome: object.derived ? object.derived.outcome : null,
      count: object.derived ? object.derived.count : null,
      txid: tx,
      at: Date.now(),
    });
  } else if (postMode === "throw") {
    throw { code: 500, message: "POC_POST_THROW" };
  } else if (postMode === "selfpatch" && Number(object.counter) < 2000) {
    openidm.patch(String(resourceName), null, [
      { operation: "replace", field: "/counter", value: 2000 + (Number(object.counter) || 0) },
    ]);
  }
})();
