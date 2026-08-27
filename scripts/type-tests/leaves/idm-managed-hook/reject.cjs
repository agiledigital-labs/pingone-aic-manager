logger.debug("object {} of {}", "alpha_user"); // expect: TS2345 — deficit
logger.info("no placeholders", "alpha_user"); // expect: TS2345 — surplus non-throwable
