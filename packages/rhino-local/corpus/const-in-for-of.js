// rhino-local-corpus
// row: const-in-for-of
// cite: docs/api/12-script-bindings-matrix.md | Language / syntax feature matrix | `const` in `for-of`
// probed: 2026-06-03
// aic-compiled: false
// aic-evaluated: false
// aic-summary: parse error
// verdict: check

var arr = [10, 20, 30];
var vals = [];
for (const v of arr) {
  vals.push(v);
}
__result = vals.join(",");
