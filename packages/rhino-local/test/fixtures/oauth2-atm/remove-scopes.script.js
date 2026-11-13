// Access-token modification conformance probe: removeScopes() returns, and AM
// then refuses to issue the token.
accessToken.setField("zz_before", "yes");
accessToken.removeScopes();
