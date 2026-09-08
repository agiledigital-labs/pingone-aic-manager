// D1 claim 2b: the obviously-broken denial. Expect the client to see 500.
function validateAccessTokenScope() {
  logger.error("AICEDIT-D1 denying via throw new Error");
  throw new Error("AICEDIT-D1 probe denial");
}
