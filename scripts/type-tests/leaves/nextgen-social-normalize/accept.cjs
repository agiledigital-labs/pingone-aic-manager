// Next-gen social profile transformation (measured 2026-11-10). The result is
// the completion value: a plain object.
var sub = rawProfile.get("sub");
var same = rawProfile["sub"];
if (sub === same && rawProfile.size() > 0 && selectedIdp === "google") {
  nodeState.putShared("provider", selectedIdp);
  nodeState.putTransient("count", 1);
}
var seen = nodeState.get("selectedIdp");
if (seen === "google") {
  logger.info("{} {}", String(rawProfile.keys()), String(requestHeaders.get("host")));
}
if (typeof existingSession !== "undefined") {
  logger.info("a session binding exists");
}
({ givenName: sub, familyName: same });
