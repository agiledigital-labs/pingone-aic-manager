// The legacy access-token-modification leaf: classic Debug method names over
// slf4j formatting. Every call here is correct at runtime.
logger.message("plain message");
logger.error("one {} placeholder", "alpha");
logger.warning("two {} and {}", "alpha", "bravo");

// `\{}` is an escape — no argument for it.
logger.error("escaped \\{} stays literal");
logger.error("escaped \\{} then {} bound", "alpha");

// `\\` is a literal backslash, so the `{}` after it IS a placeholder.
logger.error("double \\\\{} bound", "alpha");

// A trailing throwable is an argument slf4j takes without a `{}`.
try {
  accessToken.setField("x", 1);
} catch (e) {
  logger.error("could not set field", e);
  logger.error("could not set field for {}", "alpha", e);
}

// A widened message cannot be counted, so it stays unchecked.
var built = "dynamic " + String(scopes);
logger.message(built, 1, 2, 3);

// A template literal keeps its type: the visible `{}` is still counted.
logger.error(`realm ${realm} said {}`, "alpha");

if (logger.messageEnabled()) {
  logger.message("guarded {}", realm);
}
