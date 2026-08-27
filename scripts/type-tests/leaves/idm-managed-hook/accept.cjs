logger.info("hook ran");
logger.debug(
  "object {} of {}",
  openidm.read("managed/alpha_user/x"),
  "alpha_user"
);
logger.error("escaped \\{} literal");
