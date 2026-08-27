logger.message("claims for {} in {}", "alpha"); // expect: TS2345 — deficit
logger.error("no placeholders", "alpha"); // expect: TS2345 — surplus non-throwable
