// The legacy OIDC claims leaf takes rhino plus its own overlay and nothing
// else — no common.d.ts — which is why the format types live in rhino.
logger.message("claims for {}", "alpha");
logger.error("no placeholders");
logger.warning("escaped \\{} literal");

// The classic AMIdentity. `getAttribute` hands back a java.util.HashSet, so
// size/contains/toArray are the accessors and `toArray()[0]` is how a
// single-valued attribute is read (measured 2026-09-10 — docs/api/14).
var mail = identity.getAttribute("mail");
if (mail.size() > 0 && !mail.contains("")) {
  claims.get(String(mail.toArray()[0]));
}

// getAttributes() is a Map of those Sets, and its value is nullable.
var all = identity.getAttributes();
if (all.size() > 0 && all.containsKey && all.containsKey("mail")) {
  var mails = all.get("mail");
  if (mails) {
    logger.message("mail values: {}", String(mails.size()));
  }
}
