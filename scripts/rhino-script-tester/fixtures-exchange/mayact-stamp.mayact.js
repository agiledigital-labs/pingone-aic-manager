// D6 positive control. Stamps `may_act` naming the acting client, which is the
// one thing that makes an exchange legal. Its silent twin
// (`mayact-silent.mayact.js`) is the same script with this call removed, so the
// pair isolates `setMayAct` and nothing else.
//
// Context OAUTH2_MAY_ACT_NEXT_GEN, evaluatorVersion 2.0. The same body on the
// legacy context fails with `500 Error running may_act script` — legacy Rhino
// will not coerce a JS object literal into the Java map `setMayAct` wants.
token.setMayAct({ client_id: "AIC_ACTOR_CLIENT_ID" });
