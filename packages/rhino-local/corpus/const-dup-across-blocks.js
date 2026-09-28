// rhino-local-corpus
// row: const-dup-across-blocks
// cite: docs/api/12-script-bindings-matrix.md | Language / syntax feature matrix | same `const` name twice in one function
// probed: 2026-06-06
// aic-compiled: false
// aic-evaluated: false
// aic-summary: parse error (Rhino scopes const to the function for redeclaration)
// verdict: check

function run() {
  var out = [];
  {
    const dup = "first";
    out.push(dup);
  }
  {
    const dup = "second";
    out.push(dup);
  }
  return out.join(",");
}

__result = run();
