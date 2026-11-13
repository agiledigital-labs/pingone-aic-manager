// Next-gen may-act (measured 2026-11-10): a five-method token.
var existing = token.getMayAct();
if (existing !== null) {
  logger.info("already stamped");
}
token.setMayAct({ client_id: "a" });
token.setMayAct(null); // stamps JSON null
token.setAct({ sub: "b" });
logger.info("{}", String(token.getAct()));
if (scopes.contains("openid") && scopes.length > 0) {
  logger.info("scope {}", String(scopes[0]));
}
// toArray() is measured (not a JS array), so only length and [i] are declared.
logger.info("{}", String(scopes.toArray().length));
