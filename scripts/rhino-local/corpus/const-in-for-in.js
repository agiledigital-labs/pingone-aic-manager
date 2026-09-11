// rhino-local-corpus
// row: const-in-for-in
// cite: docs/api/12-script-bindings-matrix.md | Language / syntax feature matrix | `const` in `for-in`
// probed: 2026-06-03
// aic-compiled: false
// aic-evaluated: false
// aic-summary: parse error
// verdict: check

var obj = { a: 1, b: 2 };
var keys = [];
for (const k in obj) {
  keys.push(k);
}
__result = keys.join(",");
