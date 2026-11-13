// Access-token modification conformance probe: how next-gen's identity
// converts to a string. String(identity) and identity.toString() differ on
// AIC; the result rides out in the token's `aicprobe` claim.
var out = {};
try { out.str = String(identity); } catch (e) { out.str = "throw:" + String(e); }
try { out.ts = identity.toString(); } catch (e) { out.ts = "throw:" + String(e); }
try { identity.getAttribute("x"); out.ga = "no throw"; } catch (e) { out.ga = String(e); }
accessToken.setField("aicprobe", JSON.stringify(out));
