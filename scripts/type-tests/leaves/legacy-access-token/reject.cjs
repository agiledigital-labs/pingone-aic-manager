logger.error("two {} and {} placeholders", "alpha"); // expect: TS2345 — deficit
logger.error("three {} {} {}", "a", "b"); // expect: TS2345 — deficit
logger.message("one {} placeholder", "alpha", "bravo"); // expect: TS2345 — surplus non-throwable
logger.warning("no placeholders at all", "alpha"); // expect: TS2345 — surplus non-throwable
logger.error("escaped \\{} binds nothing", "alpha"); // expect: TS2345 — escaped, so no slot
logger.error("double \\\\{} bound", "a", "b"); // expect: TS2345 — surplus; the ACCEPT row is what proves parity
logger.error("boom", undefined); // expect: TS2345 — a dropped argument, not a throwable
