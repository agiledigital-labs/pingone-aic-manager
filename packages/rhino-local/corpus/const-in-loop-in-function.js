// rhino-local-corpus
// row: const-in-loop-in-function
// cite: docs/api/12-script-bindings-matrix.md | Language / syntax feature matrix | `const` in a loop body INSIDE a function
// probed: 2026-07-13
// aic-compiled: true
// aic-evaluated: true
// aic-result: 0,0,0
// aic-summary: initializer runs on the first iteration only ("0,0,0")
// verdict: check

function doubleAll() {
  var out = [];
  for (var i = 0; i < 3; i++) {
    const doubled = i * 2;
    out.push(doubled);
  }
  return out.join(",");
}

__result = doubleAll();
