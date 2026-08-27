logger.info("two {} and {}", 1); // expect: TS2345 — deficit
logger.debug("none", 1); // expect: TS2345 — surplus non-throwable
logger.error("boom", undefined); // expect: TS2345 — a dropped argument, not a throwable
