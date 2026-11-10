// OIDC claims conformance probe: does changing the `claims` binding reach the
// id token when the script returns a different map?
var nextGen = typeof require === "function";
claims.put("zzPut", "p");
claims.remove("subname");
if (nextGen) {
  claims.zzAssigned = "a";
  ({ values: { zzReturned: "r" }, compositeScopes: {} });
} else {
  var m = new java.util.LinkedHashMap();
  m.put("zzReturned", "r");
  new org.forgerock.oauth2.core.UserInfoClaims(m, new java.util.LinkedHashMap());
}
