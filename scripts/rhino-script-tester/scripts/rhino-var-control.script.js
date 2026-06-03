var result = { marker: "aic-rhino-var-control", tests: [] };

function record(name, value) {
  result.tests.push({ name: name, value: String(value) });
}

try {
  var topLevelVar = "top";
  record("topLevelVar", topLevelVar);

  function functionVar() {
    var functionScopedVar = "function";
    if (true) {
      var blockScopedVar = "block";
      record("blockScopedVar", blockScopedVar);
    }
    return functionScopedVar;
  }
  record("functionVar", functionVar());

  var loopValues = [];
  for (var i = 0; i < 3; i++) {
    loopValues.push(i);
  }
  record("forVar", loopValues.join(","));

  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify({
      ok: true,
      result: result
    }));
  }
  outcome = "ok";
} catch (e) {
  logger.error("AIC Rhino var control failed: {}", String(e));
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify({
      ok: false,
      error: String(e),
      stack: String(e && e.stack || "")
    }));
  }
  outcome = "error";
}
