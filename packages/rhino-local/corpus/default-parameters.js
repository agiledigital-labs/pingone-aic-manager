// rhino-local-corpus
// row: default-parameters
// cite: docs/api/12-script-bindings-matrix.md | Language / syntax feature matrix | default parameters `f(a, b = 2)`
// probed: 2026-06-03
// aic-compiled: false
// aic-evaluated: false
// aic-summary: parse error
// verdict: check

function withDefault(a, b = 2) {
  return a + b;
}

__result = String(withDefault(40));
