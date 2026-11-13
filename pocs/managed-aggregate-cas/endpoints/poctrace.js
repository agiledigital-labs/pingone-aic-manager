/*
 * poctrace — POC endpoint for E20 (tracing a change by transaction id).
 * Logs at each level, does one read and one query, and reports what it can
 * see of its own transaction id. Never returns or logs `context` itself.
 */
(function () {
  var tag = "POC_TRACE " + request.content.tag;
  logger.debug(tag + " debug");
  logger.info(tag + " info");
  logger.warn(tag + " warn");
  logger.error(tag + " error");
  var r = openidm.read("managed/alpha_poc_agg/" + request.content.id, null, ["_id"]);
  var q = openidm.query("managed/alpha_poc_sink", { _queryFilter: 'parent eq "' + request.content.id + '"' }, ["_id"]);
  var tx = context.transactionId;
  return {
    read: r ? r._id : null,
    queried: q.result.length,
    txType: typeof tx,
    txValue: tx && tx.transactionId ? String(tx.transactionId.value) : tx && tx.value ? String(tx.value) : null,
    txKeys: tx ? Object.keys(tx).join(",") : null,
  };
})();
