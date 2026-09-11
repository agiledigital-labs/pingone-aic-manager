// rhino-local-corpus
// row: for-of-var
// cite: docs/api/12-script-bindings-matrix.md | Language / syntax feature matrix | `for...of` itself, even with `var`
// probed: 2026-07-30
// aic-compiled: false
// aic-evaluated: false
// aic-exception-contains: missing ; after for-loop initializer
// aic-summary: parse error: missing ; after for-loop initializer
// verdict: check

var sum = 0;
for (var n of [1, 2, 3]) {
  sum += n;
}
__result = String(sum);
