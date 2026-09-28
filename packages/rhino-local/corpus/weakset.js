// rhino-local-corpus
// row: weakset
// cite: docs/api/12-script-bindings-matrix.md | ES2015 global objects | WeakSet
// probed: 2026-07-30
// aic-compiled: true
// aic-evaluated: true
// aic-result: typeof=undefined;new=ReferenceError: "WeakSet" is not defined.
// aic-summary: typeof undefined; new → ReferenceError
// verdict: check

var bits = [];
bits.push("typeof=" + typeof WeakSet);
try {
  new WeakSet();
  bits.push("new=ok");
} catch (e) {
  bits.push("new=" + String(e));
}
__result = bits.join(";");
