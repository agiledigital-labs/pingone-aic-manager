var result = { marker: "aic-rhino-let-probe", tests: [] };

function record(name, value) {
  result.tests.push({ name: name, value: String(value) });
}

try {
  let topLevelLet = "top";
  record("topLevelLet", topLevelLet);

  function functionLet() {
    let functionScopedLet = "function";
    if (true) {
      let blockScopedLet = "block";
      record("blockScopedLet", blockScopedLet);
    }
    return functionScopedLet;
  }
  record("functionLet", functionLet());

  var loopValues = [];
  for (let i = 0; i < 3; i++) {
    loopValues.push(i);
  }
  record("forLet", loopValues.join(","));

  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify({
      ok: true,
      result: result
    }));
  }
  outcome = "ok";
} catch (e) {
  logger.error("AIC Rhino let probe failed: {}", String(e));
  if (callbacks.isEmpty()) {
    callbacksBuilder.hiddenValueCallback("result", JSON.stringify({
      ok: false,
      error: String(e),
      stack: String(e && e.stack || "")
    }));
  }
  outcome = "error";
}
