// rhino-local-corpus
// row: string-normalize
// cite: docs/api/12-script-bindings-matrix.md | Language / syntax feature matrix | String.prototype.normalize
// probed: 2026-07-02
// aic-compiled: true
// aic-evaluated: true
// aic-result: function|2|Jose|Nguyen|true
// aic-summary: works (NFD/NFC + combining-mark strip)
// verdict: check
//
// Engine methods only. The AIC fixture also required a LIBRARY script; that
// binding is out of scope for this harness.

var bits = [];
bits.push(typeof "".normalize);
bits.push(String("é".normalize("NFD").length));
bits.push("José".normalize("NFD").replace(/[\u0300-\u036f]/g, ""));
bits.push("Nguyễn".normalize("NFD").replace(/[\u0300-\u036f]/g, ""));
bits.push(String("e\u0301".normalize("NFC") === "é"));
__result = bits.join("|");
