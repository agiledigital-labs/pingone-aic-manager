// Legacy (evaluatorVersion 1.0) probe: which of the classic Debug `logger`
// levels actually REACH the am-core log, held against argument shape.
//
// Companion to `legacy-logger-args.script.js`, whose first run showed six of
// seven `error()` calls producing no log line at all while `message()` and
// `warning()` both came through — so "did not throw" and "was logged" have to
// be separated before anything is claimed about `{}` binding.
//
// The grid varies exactly two things and nothing else: the LEVEL (error /
// warning / message) against the ARGUMENT SHAPE (bare / one placeholder+arg /
// surplus arg / trailing Throwable / backslash-escaped placeholder). D2 repeats
// D1 verbatim to rule out per-template dedup.
//
//   aic logs tx <txid> --source am-core | grep AICPROBE
//
// Non-destructive. Safe to delete.
var frJava = JavaImporter(
  org.forgerock.openam.auth.node.api.Action,
  com.sun.identity.authentication.callbacks.HiddenValueCallback
);

function emit(payload) {
  var json = JSON.stringify(payload);
  if (callbacks.isEmpty()) {
    action = frJava.Action.send(
      new frJava.HiddenValueCallback("result", json)
    ).build();
  } else {
    action = frJava.Action.goTo("ok").build();
  }
}

var calls = {};

function probe(id, fn) {
  try {
    fn();
    calls[id] = "ok";
  } catch (e) {
    calls[id] = "threw: " + String(e);
  }
}

try {
  // Bare single argument — the shape the old type allowed, at all three levels.
  probe("A1", function () {
    logger.error("AICPROBE-A1 error bare");
  });
  probe("A2", function () {
    logger.warning("AICPROBE-A2 warning bare");
  });
  probe("A3", function () {
    logger.message("AICPROBE-A3 message bare");
  });

  // One placeholder, one argument.
  probe("B1", function () {
    logger.error("AICPROBE-B1 error {} arg", "X");
  });
  probe("B2", function () {
    logger.warning("AICPROBE-B2 warning {} arg", "X");
  });
  probe("B3", function () {
    logger.message("AICPROBE-B3 message {} arg", "X");
  });

  // Surplus argument, no placeholder to consume it.
  probe("C1", function () {
    logger.error("AICPROBE-C1 error surplus", "X");
  });
  probe("C2", function () {
    logger.warning("AICPROBE-C2 warning surplus", "X");
  });
  probe("C3", function () {
    logger.message("AICPROBE-C3 message surplus", "X");
  });

  // Same template twice: if only one of the pair lands, the log path dedups.
  probe("D1", function () {
    logger.error("AICPROBE-D1 error repeated");
  });
  probe("D2", function () {
    logger.error("AICPROBE-D1 error repeated");
  });

  // Trailing Throwable at each level. slf4j consumes it as an exception rather
  // than as a `{}` binding, so it must NOT be counted against the placeholders
  // — the whole reason the type keeps an optional trailing slot.
  probe("E1", function () {
    logger.error(
      "AICPROBE-E1 error throwable",
      new java.lang.RuntimeException("AICPROBE-BOOM-E1")
    );
  });
  probe("E2", function () {
    logger.warning(
      "AICPROBE-E2 warning throwable",
      new java.lang.RuntimeException("AICPROBE-BOOM-E2")
    );
  });
  probe("E3", function () {
    logger.message(
      "AICPROBE-E3 message throwable",
      new java.lang.RuntimeException("AICPROBE-BOOM-E3")
    );
  });

  // Backslash-escaped placeholder. If slf4j's escape applies, the output keeps
  // a literal `{}` and drops the argument; if it does not, the argument binds.
  // Decides whether the placeholder COUNT in the type has to honour `\{}`.
  probe("F1", function () {
    logger.error("AICPROBE-F1 escaped \\{} literal", "X");
  });
  probe("F2", function () {
    logger.error("AICPROBE-F2 escaped \\{} then {} bound", "X");
  });

  // An ESCAPED BACKSLASH before a placeholder. slf4j's rule is parity: `\\`
  // is a literal backslash, so the `{}` after it is a REAL placeholder. The
  // type's escape check has to count trailing backslashes rather than look at
  // one, and this row is the only thing that says which way round it goes.
  probe("G1", function () {
    logger.error("AICPROBE-G1 double \\\\{} bound", "X");
  });

  // THE CASE THAT DECIDES THE TYPE'S SHAPE: placeholders and arguments are
  // EQUAL, and the single argument is a throwable. If slf4j strips a trailing
  // throwable BEFORE formatting — unconditionally, not only when there are
  // spare arguments — the `{}` is left unfilled and the value never appears.
  // An arity check that lets a throwable fill a placeholder slot would call
  // this correct when it is exactly the bug we are trying to catch.
  probe("H1", function () {
    logger.error(
      "AICPROBE-H1 equal count {}",
      new java.lang.RuntimeException("AICPROBE-BOOM-H1")
    );
  });
  probe("H2", function () {
    logger.error(
      "AICPROBE-H2 equal count {} and {}",
      "X",
      new java.lang.RuntimeException("AICPROBE-BOOM-H2")
    );
  });

  // A JavaScript Error, not a Java Throwable. `LoggedThrowable` may not include
  // `Error` unless slf4j actually treats one as an exception rather than as a
  // surplus argument that gets dropped.
  probe("I1", function () {
    logger.error("AICPROBE-I1 js error trailer", new Error("AICPROBE-BOOM-I1"));
  });
  probe("I2", function () {
    logger.error(
      "AICPROBE-I2 js error {} bound",
      "X",
      new Error("AICPROBE-BOOM-I2")
    );
  });

  emit({
    ok: true,
    feature: "legacy-logger-levels",
    calls: calls,
    enabled: {
      error: logger.errorEnabled(),
      message: logger.messageEnabled(),
      warning: logger.warningEnabled(),
    },
  });
} catch (e) {
  emit({ ok: false, feature: "legacy-logger-levels", error: String(e) });
}
