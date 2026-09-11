// rhino-local-corpus
// row: const-in-loop-body
// cite: docs/api/12-script-bindings-matrix.md | Language / syntax feature matrix | `const` in a for/for-in/for-of/while/do-while loop body
// probed: 2026-06-03
// aic-compiled: true
// aic-evaluated: true
// aic-result: ,,
// aic-summary: parses; value reads back undefined (",,")
// verdict: check
//
// Top-level loop in a decision-node script. AIC join() of three undefined
// values is ",,". The in-function variant is const-in-loop-in-function.js.

var out = [];
for (var i = 0; i < 3; i++) {
  const doubled = i * 2;
  out.push(doubled);
}
__result = out.join(",");
