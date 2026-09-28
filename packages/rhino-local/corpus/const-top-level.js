// rhino-local-corpus
// row: const-top-level
// cite: docs/api/12-script-bindings-matrix.md | Language / syntax feature matrix | `const` at top level (decision-node script)
// probed: 2026-06-03
// aic-compiled: true
// aic-evaluated: true
// aic-result: undefined
// aic-summary: parses but value reads back undefined
// verdict: check
//
// Decision-node top level only. LIBRARY top-level const is a different matrix
// row (works) and is not modelled here: this harness evals a source file as a
// script, which is the decision-node analogue.

const TOP_LEVEL_CONST = "const-top-level-ok";
__result = String(TOP_LEVEL_CONST);
