// rhino-local-corpus
// row: object-shorthand
// cite: docs/api/12-script-bindings-matrix.md | Language / syntax feature matrix | object shorthand `{a, b}`
// probed: 2026-06-03
// aic-compiled: false
// aic-evaluated: false
// aic-exception-contains: missing : after property id
// aic-summary: parse error: missing : after property id
// verdict: check

var a = 1;
var b = 2;
var obj = { a, b };
__result = JSON.stringify(obj);
