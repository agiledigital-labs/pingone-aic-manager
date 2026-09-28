// rhino-local-corpus
// row: object-destructuring
// cite: docs/api/12-script-bindings-matrix.md | Language / syntax feature matrix | object destructuring `var {x} = o`
// probed: 2026-06-03
// aic-compiled: false
// aic-evaluated: false
// aic-summary: parse error
// verdict: check

var src = { x: 1, y: 2 };
var { x, y } = src;
__result = x + "," + y;
