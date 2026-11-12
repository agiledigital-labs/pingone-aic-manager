// Next-gen validate scope (measured 2026-11-10). AM calls the global with no
// arguments and takes the JS array it returns as the grant.
function validateAccessTokenScope() {
  var granted = [];
  for (var i = 0; i < requestedScopes.length; i++) {
    granted.push(String(requestedScopes[i]));
  }
  // The sets are Java lists: these members are the measured ones.
  if (requestedScopes.contains("a") && requestedScopes.indexOf("a") >= 0) {
    granted = [].concat(requestedScopes);
  }
  if (!allowedScopes.isEmpty() && availableScopes.size() > 0) {
    defaultScopes.forEach(function (scope) {
      logger.info("default {}", scope);
    });
  }
  requestedScopes.add("b");
  if (granted.length === 0) {
    scopeValidatorHelper.throwInvalidScope("nothing requested");
  }
  scopeValidatorHelper.throwInvalidScope(7);
  scopeValidatorHelper.throwInvalidScope(null);
  scopeValidatorHelper.inheritAccessTokenScopesOnRefresh();
  return granted;
}
void validateAccessTokenScope;
