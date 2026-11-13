// Next-gen config provider (measured 2026-11-10). AM reads `config` after the
// script ends, and it must be a plain object.
if (nodeState.isDefined("greeting") && nodeState.get("greeting") === "hi") {
  nodeState.putShared("seen", 1);
}
if (callbacks.isEmpty()) {
  logger.info("first request {}", String(requestHeaders.get("x-probe")));
}
config = { message: { en: "hello" }, count: 1 };
config = {};
// Assign, do not declare (`var config` is an unused module local); an assigned
// config can still be changed afterwards, and the final value is used.
config = { message: { en: "hello" } };
config["count"] = 2;
