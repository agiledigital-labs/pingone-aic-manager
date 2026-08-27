logger.info("two {} and {}", 1); // expect: TS2345 — deficit
logger.debug("none", 1); // expect: TS2345 — surplus non-throwable
logger.warn("one {}", 1, 2); // expect: TS2345 — surplus non-throwable
