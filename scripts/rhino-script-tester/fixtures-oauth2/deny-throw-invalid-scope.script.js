// D1 claim 2a: the sanctioned denial. Expect the client to see 400.
function validateAccessTokenScope() {
  logger.error("AICEDIT-D1 denying via throwInvalidScope");
  scopeValidatorHelper.throwInvalidScope("AICEDIT-D1 probe denial");
  logger.error("AICEDIT-D1 UNREACHABLE: throwInvalidScope returned");
  return [];
}
