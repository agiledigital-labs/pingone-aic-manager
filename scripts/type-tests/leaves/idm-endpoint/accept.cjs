logger.info("plain");
logger.debug("one {} bound", request.method);
logger.warn("two {} and {}", 1, true);
logger.error("escaped \\{} literal");
logger.trace("double \\\\{} bound", "x");
var msg = "built " + String(request.method);
logger.info(msg, 1, 2, 3);
