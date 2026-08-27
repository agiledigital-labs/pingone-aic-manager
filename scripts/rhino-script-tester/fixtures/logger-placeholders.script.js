// Probe: does the NEXT-GEN slf4j `logger` bind `{}` placeholders from the extra
// arguments, and what happens when the counts do not line up?
//
// The next-gen `Logger` type has always declared `...args: any[]`, but the
// `{}`-binding claim behind it came from the script editor's binding metadata,
// never from a run. That claim is now load-bearing: `nextgen-common.d.ts` counts
// the `{}` in the format string and demands one argument each, so the runtime
// has to actually behave that way.
//
// Companion to `fixtures-legacy/legacy-logger-levels.script.js`, which asks the
// same grid of the legacy Debug `logger`. Same three questions, and only the
// third is answerable from inside the script:
//
//   1. binding — does `{}` get filled? (am-core log text)
//   2. mismatch — what does a deficit / surplus argument do? (am-core log text)
//   3. arity — does any of it throw? (payload `calls`)
//
//   aic logs tx <txid> --source am-core | grep AICPROBE
//
// A1 is the control: no placeholders, no arguments. If it is missing from the
// log the run says nothing about the rest — note that the log API is eventually
// consistent, so an immediate fetch can return a PARTIAL set (observed
// 2026-08-27: six of nine lines absent on the first read, all present a few
// minutes later). Re-fetch before concluding a line was never written.
//
// Non-destructive. Safe to delete.
function emit(payload) {
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify(payload));
  }
  outcome = payload.ok ? "ok" : "error";
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
  // Control: bare message at every level the next-gen logger publishes.
  probe("A1", function () {
    logger.error("AICPROBE-NG-A1 error bare");
  });
  probe("A2", function () {
    logger.warn("AICPROBE-NG-A2 warn bare");
  });
  probe("A3", function () {
    logger.info("AICPROBE-NG-A3 info bare");
  });
  probe("A4", function () {
    logger.debug("AICPROBE-NG-A4 debug bare");
  });
  probe("A5", function () {
    logger.trace("AICPROBE-NG-A5 trace bare");
  });

  // Exact match: one placeholder per argument.
  probe("B1", function () {
    logger.error("AICPROBE-NG-B1 one {} bound", "ALPHA");
  });
  probe("B2", function () {
    logger.warn("AICPROBE-NG-B2 two {} and {} bound", "ALPHA", "BRAVO");
  });
  probe("B3", function () {
    logger.info("AICPROBE-NG-B3 two {} and {} bound", "ALPHA", "BRAVO");
  });

  // Deficit: more placeholders than arguments — does the spare `{}` survive
  // into the log verbatim? That is the defect the type is meant to catch.
  probe("C1", function () {
    logger.error(
      "AICPROBE-NG-C1 three {} {} {} placeholders",
      "ALPHA",
      "BRAVO"
    );
  });

  // Surplus: an argument with no placeholder to consume it.
  probe("D1", function () {
    logger.error("AICPROBE-NG-D1 no placeholders", "ALPHA");
  });

  // Trailing Throwable, with and without a placeholder ahead of it. Next-gen
  // refuses to CONSTRUCT one (`new java.lang.RuntimeException(...)` fails with
  // `[JavaPackage java.lang.RuntimeException] is not a function`, same
  // allow-list that blocks `new java.util.HashMap()`), so the throwable has to
  // be caught from a static call instead — which is also how a real script
  // comes by one.
  //
  // The E rows are the discriminating pair: Rhino hands `catch (e)` a JS
  // WRAPPER, and the Java Throwable is underneath it. If slf4j's
  // last-argument-is-a-Throwable rule is what attaches the stack trace, E1 (the
  // wrapper) must be dropped as a surplus argument while E2 (the unwrapped
  // throwable) gets an `exception` field.
  //
  // The unwrapping property is `rhinoException`, NOT `javaException`: for an
  // error raised INSIDE the engine, `javaException` is `undefined`, so a first
  // pass that read it logged `undefined` and proved nothing. `E0-shape` records
  // both so the next reader can see which one carried the throwable.
  var caught = null;
  probe("E0", function () {
    try {
      java.lang.Integer.parseInt("AICPROBE-not-a-number");
    } catch (e) {
      caught = e;
    }
    if (caught === null) {
      throw new Error("expected parseInt to throw");
    }
    calls["E0-shape"] =
      "caught=" +
      String(caught) +
      " javaException=" +
      typeof caught.javaException +
      " rhinoException=" +
      typeof caught.rhinoException +
      " getMessage=" +
      typeof caught.getMessage;
  });
  probe("E1", function () {
    logger.error("AICPROBE-NG-E1 rhino wrapper trailer", caught);
  });
  probe("E2", function () {
    logger.error(
      "AICPROBE-NG-E2 java throwable trailer",
      caught.rhinoException
    );
  });
  probe("E3", function () {
    logger.error(
      "AICPROBE-NG-E3 {} plus java throwable",
      "ALPHA",
      caught.rhinoException
    );
  });

  // Backslash-escaped placeholder: decides whether the type's `{}` count has to
  // honour an escape.
  probe("F1", function () {
    logger.error("AICPROBE-NG-F1 escaped \\{} literal", "ALPHA");
  });
  probe("F2", function () {
    logger.error("AICPROBE-NG-F2 escaped \\{} then {} bound", "ALPHA");
  });

  emit({ ok: true, feature: "logger-placeholders", calls: calls });
} catch (e) {
  emit({ ok: false, feature: "logger-placeholders", error: String(e) });
}
