logger.info("plain");
logger.debug("one {} bound", nodeState.get("username"));
logger.warn("two {} and {}", 1, true);
logger.trace("escaped \\{} literal");
logger.error("double \\\\{} bound", "x");
var msg = "built at " + scriptName;
logger.error(msg, 1, 2);
logger.info(`user ${realm} said {}`, "hi");
