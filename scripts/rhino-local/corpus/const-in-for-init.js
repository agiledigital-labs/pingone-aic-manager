// rhino-local-corpus
// row: const-in-for-init
// cite: docs/api/12-script-bindings-matrix.md | Language / syntax feature matrix | `const` in `for` init
// probed: 2026-06-03
// aic-compiled: false
// aic-evaluated: false
// aic-summary: parse error
// verdict: check

var seen = [];
for (const i = 0; i < 3; i++) {
  seen.push(i);
}
__result = seen.join(",");
