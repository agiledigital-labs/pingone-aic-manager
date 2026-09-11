// rhino-local-corpus
// row: const-in-function
// cite: docs/api/12-script-bindings-matrix.md | Language / syntax feature matrix | `const` in a function body
// probed: 2026-06-03
// aic-compiled: true
// aic-evaluated: true
// aic-result: const-in-function-ok
// aic-summary: works, correct value
// verdict: check

function useConst() {
  const FN_SCOPED = "const-in-function-ok";
  return FN_SCOPED;
}

__result = useConst();
