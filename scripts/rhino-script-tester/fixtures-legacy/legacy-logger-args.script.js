// Legacy (evaluatorVersion 1.0) probe: does the classic Debug `logger` accept
// MORE THAN ONE argument, and does it substitute slf4j `{}` placeholders?
//
// The type layer declared `message`/`error`/`warning` as single-argument
// (legacy-common.d.ts), which rejects the two-argument calls real scripts make.
// This fixture answers three separate questions, because "it did not throw" is
// not the same as "the argument was used":
//
//   1. arity   — does an extra argument throw? (payload `calls`)
//   2. binding — does `{}` get filled from the extra args? (am-core log text)
//   3. surplus — what happens to an argument with no `{}` to fill?
//
// Question 2 and 3 can only be answered from the LOG BODY, so every call is
// tagged `AICPROBE-nn` and the run's transaction id is emitted alongside:
//
//   aic logs tx <txid> --source am-core | grep AICPROBE
//
// The zero-argument control (`AICPROBE-01`) is what proves the log path works
// at all — without it, an empty grep is ambiguous between "no substitution"
// and "nothing logged".
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
  // --- error() -----------------------------------------------------------
  // 01 is the control: one argument, the shape the old type allowed.
  probe("01-error-control", function () {
    logger.error("AICPROBE-01 control single argument");
  });
  // 02/03: the discriminating cases — if `{}` is NOT a placeholder the log
  // line comes out with the braces still in it and ALPHA/BRAVO absent.
  probe("02-error-one-placeholder", function () {
    logger.error("AICPROBE-02 one {} placeholder", "ALPHA");
  });
  probe("03-error-two-placeholders", function () {
    logger.error("AICPROBE-03 two {} and {} placeholders", "ALPHA", "BRAVO");
  });
  // 04: surplus argument, no placeholder to consume it.
  probe("04-error-surplus-arg", function () {
    logger.error("AICPROBE-04 no placeholders", "ALPHA");
  });
  // 05: too few arguments for the placeholders — does the third `{}` survive?
  probe("05-error-too-few-args", function () {
    logger.error("AICPROBE-05 three {} {} {} placeholders", "ALPHA", "BRAVO");
  });
  // 06: the classic Debug (String, Throwable) overload, which is why the type
  // cannot simply demand exactly one argument per `{}`.
  probe("06-error-throwable", function () {
    logger.error(
      "AICPROBE-06 throwable trailer",
      new java.lang.RuntimeException("AICPROBE-BOOM")
    );
  });
  // 07: placeholder AND a trailing throwable together.
  probe("07-error-placeholder-and-throwable", function () {
    logger.error(
      "AICPROBE-07 {} plus throwable",
      "ALPHA",
      new java.lang.RuntimeException("AICPROBE-BOOM7")
    );
  });

  // --- message() / warning() ---------------------------------------------
  // Same questions on the other two Debug levels. `message` is debug-level and
  // may be filtered out of am-core entirely, hence the *Enabled readings below.
  probe("08-message-two-placeholders", function () {
    logger.message("AICPROBE-08 two {} and {} placeholders", "ALPHA", "BRAVO");
  });
  probe("09-warning-two-placeholders", function () {
    logger.warning("AICPROBE-09 two {} and {} placeholders", "ALPHA", "BRAVO");
  });

  emit({
    ok: true,
    feature: "legacy-logger-args",
    calls: calls,
    enabled: {
      error: logger.errorEnabled(),
      message: logger.messageEnabled(),
      warning: logger.warningEnabled(),
    },
  });
} catch (e) {
  emit({ ok: false, feature: "legacy-logger-args", error: String(e) });
}
