// rhino-local-corpus
// row: arrow-functions
// cite: docs/api/12-script-bindings-matrix.md | Language / syntax feature matrix | arrow functions `=>`
// probed: 2026-06-03
// aic-compiled: true
// aic-evaluated: true
// aic-result: 42,42
// aic-summary: works
// verdict: check

var dbl = (n) => n * 2;
var add = (a, b) => {
  return a + b;
};
__result = dbl(21) + "," + add(40, 2);
