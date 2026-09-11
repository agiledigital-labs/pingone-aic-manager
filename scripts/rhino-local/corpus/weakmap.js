// rhino-local-corpus
// row: weakmap
// cite: docs/api/12-script-bindings-matrix.md | ES2015 global objects | WeakMap
// probed: 2026-07-30
// aic-compiled: true
// aic-evaluated: true
// aic-result: typeof=undefined;new=ReferenceError: "WeakMap" is not defined.
// aic-summary: typeof undefined; new → ReferenceError
// verdict: check

var bits = [];
bits.push("typeof=" + typeof WeakMap);
try {
  new WeakMap();
  bits.push("new=ok");
} catch (e) {
  bits.push("new=" + String(e));
}
__result = bits.join(";");
